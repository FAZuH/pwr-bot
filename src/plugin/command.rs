//! Converts plugin manifest command blobs into routing [`poise::Command`]s.
//!
//! Each [`Manifest`] entry carries a Discord-native `CreateCommand` JSON blob
//! ([`CommandDef::create_command`]) — the single source of truth for both
//! Discord registration and host-side argument re-parsing. This module parses
//! such a blob into a [`HostCommandSpec`] and builds a framework `Command`
//! from it, so plugins declare slash commands without host-side per-command
//! code.
//!
//! Blob → `Command` mapping:
//! - `name`, `description` → `Command::name`, `Command::description`
//! - `options` → leaf rows become `Command::parameters`; subcommand/group
//!   rows become the `Command::subcommands` tree with `subcommand_required`
//!   on parents and `parameters` dropped (mirrors poise registration)
//! - `default_member_permissions` (Discord string bitfield) →
//!   `Command::default_member_permissions`
//! - `nsfw` → `Command::nsfw_only`; `guild_only`/`dm_only`/`ephemeral` are
//!   host extensions on the same-named fields
//! - `contexts` → `Command::interaction_context` (overrides `dm_permission`);
//!   `dm_permission: false` → `[Guild]` only when no `contexts` are given
//! - `integration_types` → `Command::install_context`
//! - per option: `type` → `CommandParameter::type_setter` (kind only; choices
//!   keep kind Integer without the min/max clamp, which Discord rejects);
//!   `required` → `CommandParameter::required`; `choices` → labels transported
//!   as integer indices (`label = choices[i]`) on choice-eligible kinds
//!   (String/Integer/Number) and dropped with a warning elsewhere — Discord
//!   rejects choices on any other kind; `channel_types` →
//!   `CommandParameter::channel_types`; `min_value`/`max_value`/
//!   `min_length`/`max_length`/`autocomplete` are parsed into [`OptionSpec`]
//!   but not applied — `type_setter` is a non-capturing function pointer and
//!   cannot ride per-option values (deferred to #114, documented).
//!
//! Deferred seams:
//! - Every built command carries [`plugin_slash_dispatch`] as its action,
//!   which routes the invocation to its plugin's view session via the
//!   command-name → plugin-name route table built from loaded manifests
//!   ([`routes_from_manifests`]; see the function's docs).
//! - Merging built commands into the framework before
//!   `Framework::builder().build()` is later work (#113);
//!   [`commands_from_manifest`] documents that call site. The bot-side
//!   registry that tracks per-plugin commands is #113 as well.
//! - [`register_in_guild`] is wired by the `/plugins` command (#113).
//! - Per-option autocomplete and min/max/length constraints are #114.

use std::borrow::Cow;
use std::collections::HashMap;

use log::warn;
use poise::Command;
use poise::CommandParameter;
use poise::CommandParameterChoice;
use poise::serenity_prelude as serenity;
use pwr_plugin_protocol::Manifest;
use serde_json::Value;

use crate::bot::Data;
use crate::bot::command::Error;
use crate::plugin::validate_view_data;

/// A parsed `CreateCommand` blob, before poise mapping.
#[derive(Debug, Clone, PartialEq)]
pub struct HostCommandSpec {
    /// Command name, e.g. `feed.list`.
    pub name: String,
    /// One-line description shown in Discord.
    pub description: String,
    /// Option rows: leaf parameters and/or subcommand/group rows.
    pub options: Vec<OptionSpec>,
    /// Default member permissions as a Discord bitfield, if given.
    pub default_member_permissions: Option<serenity::Permissions>,
    /// Whether the command may only run in DMs (host extension).
    pub dm_only: bool,
    /// Whether the command may only run in NSFW channels.
    pub nsfw_only: bool,
    /// Whether the command may only run in guilds (host extension).
    pub guild_only: bool,
    /// Whether responses should be ephemeral by default (host extension).
    pub ephemeral: bool,
    /// Installation contexts (guild/user install).
    pub install_context: Option<Vec<serenity::InstallationContext>>,
    /// Interaction contexts (guild/bot DM/private channel).
    pub interaction_context: Option<Vec<serenity::InteractionContext>>,
}

impl HostCommandSpec {
    /// Parses a Discord-native `CreateCommand` blob. Hard errors only on a
    /// non-object blob or a missing string `name`/`description`; malformed
    /// option rows are skipped and bad optional fields degrade to defaults.
    pub fn parse(blob: &Value) -> Result<HostCommandSpec, CommandSpecError> {
        let object = blob.as_object().ok_or(CommandSpecError::NotAnObject)?;
        let name = object
            .get("name")
            .and_then(Value::as_str)
            .ok_or(CommandSpecError::MissingName)?
            .to_string();
        let description = object
            .get("description")
            .and_then(Value::as_str)
            .ok_or(CommandSpecError::MissingDescription)?
            .to_string();

        let dm_permission = match object.get("dm_permission") {
            Some(Value::Bool(dm_permission)) => Some(*dm_permission),
            _ => None,
        };
        let contexts = parse_enum_list(object.get("contexts"), map_interaction_context);
        let interaction_context = match contexts {
            Some(contexts) => Some(contexts),
            None if dm_permission == Some(false) => Some(vec![serenity::InteractionContext::Guild]),
            None => None,
        };

        Ok(HostCommandSpec {
            name,
            description,
            options: parse_options(object.get("options"))?,
            default_member_permissions: object
                .get("default_member_permissions")
                .and_then(Value::as_str)
                .and_then(|bits| bits.parse::<u64>().ok())
                .map(serenity::Permissions::from_bits_truncate),
            dm_only: bool_flag(object, "dm_only"),
            nsfw_only: bool_flag(object, "nsfw"),
            guild_only: bool_flag(object, "guild_only"),
            ephemeral: bool_flag(object, "ephemeral"),
            install_context: parse_enum_list(
                object.get("integration_types"),
                map_installation_context,
            ),
            interaction_context,
        })
    }
}

/// A parsed option row. Leaf rows map to `CommandParameter`s; subcommand/group
/// rows recurse through [`OptionSpec::options`].
#[derive(Debug, Clone, PartialEq)]
pub struct OptionSpec {
    /// Option name, e.g. `url`.
    pub name: String,
    /// Option description; Discord requires one for slash parameters.
    pub description: Option<String>,
    /// Discord option type.
    pub kind: OptionKind,
    /// Whether the user must provide this option.
    pub required: bool,
    /// Choice labels, index-transported: `label = choices[i]`.
    pub choices: Vec<String>,
    /// Channel types a channel option accepts.
    pub channel_types: Vec<serenity::ChannelType>,
    /// Minimum numeric value (parsed, not applied — see module docs).
    pub min_value: Option<serde_json::Number>,
    /// Maximum numeric value (parsed, not applied — see module docs).
    pub max_value: Option<serde_json::Number>,
    /// Minimum string length (parsed, not applied — see module docs).
    pub min_length: Option<u16>,
    /// Maximum string length (parsed, not applied — see module docs).
    pub max_length: Option<u16>,
    /// Whether the option supports autocomplete (parsed, not applied — #114).
    pub autocomplete: bool,
    /// Nested rows: parameters of a subcommand, or subcommands of a group.
    pub options: Vec<OptionSpec>,
}

/// The Discord option types this host maps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptionKind {
    /// `1`: a subcommand row.
    SubCommand,
    /// `2`: a subcommand group row.
    SubCommandGroup,
    /// `3`: a string option.
    String,
    /// `4`: an integer option.
    Integer,
    /// `10`: a float option.
    Number,
    /// `5`: a boolean option.
    Boolean,
    /// `6`: a user option.
    User,
    /// `7`: a channel option.
    Channel,
    /// `8`: a role option.
    Role,
    /// `9`: a user-or-role option.
    Mentionable,
    /// `11`: an attachment option.
    Attachment,
}

impl OptionKind {
    /// Maps a Discord `type` value to an [`OptionKind`]; unknown values are
    /// rejected rather than degraded.
    pub fn from_discord(kind: i64) -> Option<Self> {
        Some(match kind {
            1 => Self::SubCommand,
            2 => Self::SubCommandGroup,
            3 => Self::String,
            4 => Self::Integer,
            5 => Self::Boolean,
            6 => Self::User,
            7 => Self::Channel,
            8 => Self::Role,
            9 => Self::Mentionable,
            10 => Self::Number,
            11 => Self::Attachment,
            _ => return None,
        })
    }

    /// Whether this kind builds into the subcommand tree rather than into
    /// `parameters`.
    fn is_subcommand(self) -> bool {
        matches!(self, Self::SubCommand | Self::SubCommandGroup)
    }

    /// Whether Discord allows choices on this kind (String/Integer/Number
    /// only); choices on any other kind are dropped at build time.
    fn supports_choices(self) -> bool {
        matches!(self, Self::String | Self::Integer | Self::Number)
    }

    /// Human-readable name of the expected resolved value, for error messages.
    fn expected_type(self) -> &'static str {
        match self {
            Self::SubCommand => "subcommand",
            Self::SubCommandGroup => "subcommand group",
            Self::String => "string",
            Self::Integer => "integer",
            Self::Number => "number",
            Self::Boolean => "boolean",
            Self::User => "user",
            Self::Channel => "channel",
            Self::Role => "role",
            Self::Mentionable => "user or role",
            Self::Attachment => "attachment",
        }
    }
}

/// Why a [`HostCommandSpec::parse`] or [`command_from_blob`] failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CommandSpecError {
    /// The blob is not a JSON object.
    #[error("command blob is not a JSON object")]
    NotAnObject,
    /// The blob is missing a string `name`.
    #[error("command blob is missing a string `name`")]
    MissingName,
    /// The blob is missing a string `description`.
    #[error("command blob is missing a string `description`")]
    MissingDescription,
    /// An option row carries an unknown `type` value.
    #[error("option {index} `{name}` has unknown type {kind}")]
    UnknownOptionType {
        /// Index of the offending row within its `options` array.
        index: usize,
        /// Name of the offending option.
        name: String,
        /// The unknown `type` value.
        kind: i64,
    },
}

/// A re-parsed argument value, keyed by option name in [`reparse_args`].
#[derive(Debug, Clone, PartialEq)]
pub enum ArgValue {
    /// A string argument.
    String(String),
    /// An integer argument.
    Integer(i64),
    /// A float argument.
    Number(f64),
    /// A boolean argument.
    Boolean(bool),
    /// The label of the chosen choice (`choices[i]`).
    Choice(String),
    /// A channel argument's id.
    ChannelId(u64),
    /// A user argument's id.
    UserId(u64),
    /// A role argument's id.
    RoleId(u64),
    /// An attachment argument's id.
    AttachmentId(u64),
}

/// Why [`reparse_args`] could not reconstruct the arguments.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ReparseError {
    /// A required option was not present in the interaction.
    #[error("required argument `{name}` is missing")]
    MissingRequired {
        /// Name of the missing option.
        name: String,
    },
    /// The interaction carried a different value type than the option expects.
    #[error("argument `{name}` expected {expected}, found {found}")]
    TypeMismatch {
        /// Name of the option.
        name: String,
        /// What the option expects, e.g. `integer`.
        expected: &'static str,
        /// What the interaction carried, e.g. `string`.
        found: &'static str,
    },
    /// A choice option received an index outside its choices list.
    #[error("choice index {index} for argument `{name}` is out of range (0..{count})")]
    ChoiceIndexOutOfRange {
        /// Name of the option.
        name: String,
        /// The received index.
        index: i64,
        /// Number of choices.
        count: usize,
    },
}

/// Parses a `CreateCommand` blob into a routing poise command.
///
/// The command's `slash_action` is the core-plugin dispatch
/// ([`plugin_slash_dispatch`]); `on_error`/`checks` are left at framework
/// defaults.
pub fn command_from_blob(blob: &Value) -> Result<Command<Data, Error>, CommandSpecError> {
    let spec = HostCommandSpec::parse(blob)?;
    Ok(build_command(&spec))
}

/// Builds a routing command for every valid blob in a manifest. Invalid blobs
/// are logged and skipped so one bad plugin entry cannot take down the rest.
///
/// The returned list is the merge seam: hand it to the framework before
/// `Framework::builder().build()` (or fold it into the cog list). Wiring that
/// merge — and tracking which commands belong to which plugin — is #113.
pub fn commands_from_manifest(manifest: &Manifest) -> Vec<Command<Data, Error>> {
    let mut commands = Vec::new();
    for def in &manifest.commands {
        match command_from_blob(&def.create_command) {
            Ok(command) => commands.push(command),
            Err(error) => {
                warn!(
                    "skipping invalid command in manifest `{}`: {error}",
                    manifest.name
                );
            }
        }
    }
    commands
}

/// Re-parses an interaction's resolved options against a command's option
/// specs, returning values keyed by option name.
///
/// Mirrors poise's `SlashArgument` semantics: options are matched by name,
/// unknown interaction args are ignored, absent optional args are skipped, and
/// absent required args fail. Choice options resolve to the chosen label — by
/// index for Integer/Number, by value for String; [`ArgValue::Choice`] carries
/// the label itself.
pub fn reparse_args(
    specs: &[OptionSpec],
    args: &[serenity::ResolvedOption<'_>],
) -> Result<HashMap<String, ArgValue>, ReparseError> {
    let mut values = HashMap::with_capacity(specs.len());
    for spec in specs {
        match args.iter().find(|resolved| resolved.name == spec.name) {
            Some(resolved) => {
                values.insert(spec.name.clone(), extract_value(spec, resolved)?);
            }
            None if spec.required => {
                return Err(ReparseError::MissingRequired {
                    name: spec.name.clone(),
                });
            }
            None => {} // optional and absent: skipped
        }
    }
    Ok(values)
}

/// Extracts one argument value from a resolved interaction option.
fn extract_value(
    spec: &OptionSpec,
    resolved: &serenity::ResolvedOption<'_>,
) -> Result<ArgValue, ReparseError> {
    let mismatch = || ReparseError::TypeMismatch {
        name: spec.name.clone(),
        expected: spec.kind.expected_type(),
        found: describe(&resolved.value),
    };
    match (spec.kind, &resolved.value) {
        (OptionKind::String, serenity::ResolvedValue::String(value))
            if !spec.choices.is_empty() =>
        {
            Ok(ArgValue::Choice(value.to_string()))
        }
        (OptionKind::String, serenity::ResolvedValue::String(value)) => {
            Ok(ArgValue::String(value.to_string()))
        }
        (OptionKind::Integer, serenity::ResolvedValue::Integer(value))
            if spec.choices.is_empty() =>
        {
            Ok(ArgValue::Integer(*value))
        }
        (OptionKind::Integer, serenity::ResolvedValue::Integer(value)) => {
            let index = *value;
            let label = spec.choices.get(index as usize).ok_or_else(|| {
                ReparseError::ChoiceIndexOutOfRange {
                    name: spec.name.clone(),
                    index,
                    count: spec.choices.len(),
                }
            })?;
            Ok(ArgValue::Choice(label.clone()))
        }
        (OptionKind::Number, serenity::ResolvedValue::Number(value))
            if !spec.choices.is_empty() =>
        {
            let index = *value as i64;
            let label = spec.choices.get(index as usize).ok_or_else(|| {
                ReparseError::ChoiceIndexOutOfRange {
                    name: spec.name.clone(),
                    index,
                    count: spec.choices.len(),
                }
            })?;
            Ok(ArgValue::Choice(label.clone()))
        }
        (OptionKind::Number, serenity::ResolvedValue::Number(value)) => {
            Ok(ArgValue::Number(*value))
        }
        (OptionKind::Boolean, serenity::ResolvedValue::Boolean(value)) => {
            Ok(ArgValue::Boolean(*value))
        }
        (OptionKind::Channel, serenity::ResolvedValue::Channel(channel)) => {
            Ok(ArgValue::ChannelId(channel.id().get()))
        }
        (OptionKind::User, serenity::ResolvedValue::User(user, _)) => {
            Ok(ArgValue::UserId(user.id.get()))
        }
        (OptionKind::Role, serenity::ResolvedValue::Role(role)) => {
            Ok(ArgValue::RoleId(role.id.get()))
        }
        (OptionKind::Attachment, serenity::ResolvedValue::Attachment(attachment)) => {
            Ok(ArgValue::AttachmentId(attachment.id.get()))
        }
        (OptionKind::Mentionable, serenity::ResolvedValue::User(user, _)) => {
            Ok(ArgValue::UserId(user.id.get()))
        }
        (OptionKind::Mentionable, serenity::ResolvedValue::Role(role)) => {
            Ok(ArgValue::RoleId(role.id.get()))
        }
        _ => Err(mismatch()),
    }
}

/// Human-readable name of a resolved value, for [`ReparseError::TypeMismatch`].
fn describe(value: &serenity::ResolvedValue<'_>) -> &'static str {
    match value {
        serenity::ResolvedValue::Boolean(_) => "boolean",
        serenity::ResolvedValue::Integer(_) => "integer",
        serenity::ResolvedValue::Number(_) => "number",
        serenity::ResolvedValue::String(_) => "string",
        serenity::ResolvedValue::SubCommand(_) => "subcommand",
        serenity::ResolvedValue::SubCommandGroup(_) => "subcommand group",
        serenity::ResolvedValue::Attachment(_) => "attachment",
        serenity::ResolvedValue::Channel(_) => "channel",
        serenity::ResolvedValue::Role(_) => "role",
        serenity::ResolvedValue::User(_, _) => "user",
        serenity::ResolvedValue::Unresolved(_) => "unresolved",
        serenity::ResolvedValue::Autocomplete { .. } => "autocomplete",
        _ => "unknown",
    }
}

/// Descends an interaction's top-level options to the leaf argument list,
/// mirroring poise's `find_matching_command`: while any option at the current
/// level is a `SubCommand`/`SubCommandGroup` value, its inner list becomes the
/// new level. `ApplicationContext::args` is the leaf list.
pub fn leaf_options<'a>(
    options: &'a [serenity::ResolvedOption<'a>],
) -> &'a [serenity::ResolvedOption<'a>] {
    let mut current = options;
    loop {
        let mut next = None;
        for option in current {
            match &option.value {
                serenity::ResolvedValue::SubCommand(inner)
                | serenity::ResolvedValue::SubCommandGroup(inner) => {
                    next = Some(&**inner);
                    break;
                }
                _ => {}
            }
        }
        match next {
            Some(inner) => current = inner,
            None => return current,
        }
    }
}

/// Re-parses a slash interaction's options against a command's option schema,
/// returning the args payload the plugin receives.
///
/// The schema is the leaf [`OptionSpec`]s carried in the command's
/// `custom_data` (set from the `CreateCommand` blob at build time); the
/// interaction's top-level options are descended to the leaf list via
/// [`leaf_options`], then resolved with [`reparse_args`]. A command not built
/// by this module carries no specs and yields an empty object.
pub fn reparse_command_args(
    command: &Command<Data, Error>,
    interaction: &serenity::CommandInteraction,
) -> Result<Value, ReparseError> {
    let Some(specs) = command.custom_data.downcast_ref::<Vec<OptionSpec>>() else {
        return Ok(Value::Object(serde_json::Map::new()));
    };
    let values = reparse_args(specs, leaf_options(&interaction.data.options()))?;
    Ok(Value::Object(
        values
            .into_iter()
            .map(|(name, value)| (name, arg_value_to_json(value)))
            .collect(),
    ))
}

/// Serializes one re-parsed argument into its plugin wire value: choices
/// carry the chosen label; ids ride as numbers.
fn arg_value_to_json(value: ArgValue) -> Value {
    match value {
        ArgValue::String(text) => Value::String(text),
        ArgValue::Integer(number) => Value::from(number),
        ArgValue::Number(number) => Value::from(number),
        ArgValue::Boolean(flag) => Value::Bool(flag),
        ArgValue::Choice(label) => Value::String(label),
        ArgValue::ChannelId(id)
        | ArgValue::UserId(id)
        | ArgValue::RoleId(id)
        | ArgValue::AttachmentId(id) => Value::from(id),
    }
}

/// Selects the commands whose names appear in `names`, preserving the order
/// of `commands`. An empty selection yields an empty list.
pub fn select_commands<'a>(
    commands: &'a [Command<Data, Error>],
    names: &[&str],
) -> Vec<&'a Command<Data, Error>> {
    commands
        .iter()
        .filter(|command| names.contains(&command.name.as_ref()))
        .collect()
}

/// Registers `commands` in a guild via Discord's bulk-overwrite endpoint.
///
/// Thin wrapper over [`poise::builtins::register_in_guild`] with the slice
/// signature the `/plugins` command (#113) will call. An empty slice
/// unregisters every plugin command in the guild.
pub async fn register_in_guild(
    http: &serenity::Http,
    commands: &[Command<Data, Error>],
    guild_id: serenity::GuildId,
) -> Result<(), serenity::Error> {
    poise::builtins::register_in_guild(http, commands.iter(), guild_id).await
}

/// Command-name → plugin-name routes. Built from the manifests of the plugins
/// loaded at startup ([`routes_from_manifests`]); [`plugin_slash_dispatch`]
/// looks an invoked command name up here to find its owning plugin.
pub type PluginRoutes = HashMap<String, String>;

/// Builds the command-name → plugin-name route table from per-plugin
/// manifests. Plugins with a `None` manifest (spawned without one) and
/// command blobs [`HostCommandSpec::parse`] rejects are skipped — the same
/// acceptance the framework uses to build the commands themselves, so a
/// route exists exactly when its command was registered.
pub fn routes_from_manifests(
    manifests: impl IntoIterator<Item = (String, Option<Manifest>)>,
) -> PluginRoutes {
    let mut routes = PluginRoutes::new();
    for (name, manifest) in manifests {
        let Some(manifest) = manifest else {
            continue;
        };
        for command in &manifest.commands {
            if let Ok(spec) = HostCommandSpec::parse(&command.create_command) {
                routes.insert(spec.name, name.clone());
            }
        }
    }
    routes
}

/// Routes a slash invocation to its owning plugin's view session.
///
/// The invoked command name is looked up in the plugin route table built from
/// the loaded manifests ([`routes_from_manifests`]); unknown commands fail
/// with a structure mismatch (a `slash_action` is shared by every built
/// command, so this guard keeps unregistered commands honest). The
/// interaction's arguments are re-parsed against the command's schema
/// ([`reparse_command_args`]) so the plugin receives the real payload instead
/// of an empty object. The plugin handle comes from the manager, the initial
/// render is invoke+placeholder+edit: invoke the plugin for its spec first
/// (so the loading reply can carry `spec.ephemeral`), register the engine
/// session on the reply's message id, then replace the message body with the
/// returned spec's raw data via a bare HTTP edit — `serenity::Component` is
/// not `Deserialize`, so the spec cannot ride a typed `CreateReply`.
fn plugin_slash_dispatch(
    ctx: poise::ApplicationContext<'_, Data, Error>,
) -> poise::BoxFuture<'_, Result<(), poise::FrameworkError<'_, Data, Error>>> {
    Box::pin(async move {
        let command_name = ctx.command.name.as_ref();
        let data = ctx.framework.user_data();
        let Some(plugin_name) = data.plugin_routes.get(command_name) else {
            return Err(poise::FrameworkError::new_command_structure_mismatch(
                ctx,
                "command has no plugin route",
            ));
        };
        // Re-parse the interaction's args against the command's schema before
        // any side effects, so the plugin receives the real arguments.
        let args = match reparse_command_args(ctx.command, ctx.interaction) {
            Ok(args) => args,
            Err(error) => return Err(poise::FrameworkError::new_command(ctx.into(), error.into())),
        };
        let data = ctx.framework.user_data();
        let Some(plugin) = data.plugin_manager.get(plugin_name).await else {
            return Err(poise::FrameworkError::new_command(
                ctx.into(),
                anyhow::anyhow!("plugin `{plugin_name}` is not running").into(),
            ));
        };
        // The interaction token outlives the `Context` conversion below and is
        // needed to edit the reply through the interaction-webhook route.
        let interaction_token = ctx.interaction.token.as_str();
        let ctx: poise::Context<'_, Data, Error> = ctx.into();

        // Invoke the plugin before sending anything, so the initial reply can
        // carry the view's ephemeral flag (Discord only honors ephemeral on
        // the first interaction response).
        let spec = data
            .plugin_engine
            .invoke(plugin.clone(), command_name, args)
            .await
            .map_err(|error| poise::FrameworkError::new_command(ctx, error.into()))?;
        validate_view_data(&spec.data)
            .map_err(|error| poise::FrameworkError::new_command(ctx, error.into()))?;
        let reply = ctx
            .send(
                poise::CreateReply::new()
                    .content("Loading…")
                    .ephemeral(spec.ephemeral),
            )
            .await
            .map_err(|error| poise::FrameworkError::new_command(ctx, error.into()))?;
        let message_id = reply
            .message()
            .await
            .map_err(|error| poise::FrameworkError::new_command(ctx, error.into()))?
            .id;
        data.plugin_engine
            .register(message_id, plugin, command_name, spec.clone())
            .await;
        // Ephemeral replies cannot be edited via the channel-message route
        // (`PATCH /channels/{id}/messages/{id}` returns Unknown Message for
        // ephemeral messages); the interaction-webhook route is the only one
        // that works, for both ephemeral and public replies. The message id
        // fetched above is still needed for the engine session registration.
        ctx.http()
            .edit_original_interaction_response(interaction_token, &spec.data, Vec::new())
            .await
            .map_err(|error| poise::FrameworkError::new_command(ctx, error.into()))?;
        Ok(())
    })
}

/// Builds a routing `Command` from a parsed spec. Subcommand/group rows become
/// the `subcommands` tree (with `subcommand_required` on parents and
/// `parameters` dropped); leaf rows become `parameters`.
fn build_command(spec: &HostCommandSpec) -> Command<Data, Error> {
    // Subcommand/group rows become the `subcommands` tree; Discord allows a
    // command to carry either parameters or subcommands, never both, so leaf
    // rows are dropped when any subcommand exists (mirrors poise).
    let subcommands = spec
        .options
        .iter()
        .filter(|option| option.kind.is_subcommand())
        .map(build_subcommand)
        .collect::<Vec<_>>();
    let parameters = if subcommands.is_empty() {
        spec.options.iter().map(build_parameter).collect()
    } else {
        Vec::new()
    };
    let subcommand_required = !subcommands.is_empty();
    // The leaf option specs ride in `custom_data` for the dispatch to
    // re-parse interaction args against: the parameter kinds are unrecoverable
    // from `CommandParameter` after build (type_setter is a function pointer).
    let leaf_specs = if subcommands.is_empty() {
        spec.options.clone()
    } else {
        Vec::new()
    };

    Command {
        name: Cow::Owned(spec.name.clone()),
        description: Some(Cow::Owned(spec.description.clone())),
        subcommands,
        subcommand_required,
        parameters,
        custom_data: Box::new(leaf_specs),
        slash_action: Some(plugin_slash_dispatch),
        guild_only: spec.guild_only,
        dm_only: spec.dm_only,
        default_member_permissions: spec
            .default_member_permissions
            .unwrap_or_else(serenity::Permissions::empty),
        nsfw_only: spec.nsfw_only,
        ephemeral: spec.ephemeral,
        install_context: spec.install_context.clone().map(Cow::Owned),
        interaction_context: spec.interaction_context.clone().map(Cow::Owned),
        ..Default::default()
    }
}

/// Builds a subcommand or subcommand-group node from an option row.
fn build_subcommand(option: &OptionSpec) -> Command<Data, Error> {
    // A node carries either parameters or subcommands, never both (Discord
    // rejects a mixed row); leaf rows are dropped when any subcommand exists,
    // mirroring `build_command`.
    let subcommands = option
        .options
        .iter()
        .filter(|child| child.kind.is_subcommand())
        .map(build_subcommand)
        .collect::<Vec<_>>();
    let parameters = if subcommands.is_empty() {
        option
            .options
            .iter()
            .filter(|child| !child.kind.is_subcommand())
            .map(build_parameter)
            .collect()
    } else {
        Vec::new()
    };
    let subcommand_required = !subcommands.is_empty();
    // Same `custom_data` contract as `build_command`: this node's leaf specs,
    // for the dispatch's arg re-parse when this subcommand is invoked.
    let leaf_specs = if subcommands.is_empty() {
        option
            .options
            .iter()
            .filter(|child| !child.kind.is_subcommand())
            .cloned()
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    Command {
        name: Cow::Owned(option.name.clone()),
        description: Some(Cow::Owned(option.description.clone().unwrap_or_default())),
        subcommands,
        subcommand_required,
        parameters,
        custom_data: Box::new(leaf_specs),
        slash_action: Some(plugin_slash_dispatch),
        ..Default::default()
    }
}

/// Builds a `CommandParameter` from a leaf option row.
fn build_parameter(option: &OptionSpec) -> CommandParameter<Data, Error> {
    // Discord rejects choices on any kind but String/Integer/Number, so
    // choices on other kinds are dropped at build; re-parse only resolves
    // choices for those same kinds (see module docs).
    let choices = if option.kind.supports_choices() {
        option.choices.clone()
    } else {
        if !option.choices.is_empty() {
            warn!(
                "dropping {} choices on `{}` option: Discord only allows choices on \
                 string, integer, or number options",
                option.choices.len(),
                option.name,
            );
        }
        Vec::new()
    };
    CommandParameter {
        name: Cow::Owned(option.name.clone()),
        description: Some(Cow::Owned(option.description.clone().unwrap_or_default())),
        name_localizations: Cow::Owned(Vec::new()),
        description_localizations: Cow::Owned(Vec::new()),
        required: option.required,
        type_setter: Some(type_setter_for(option.kind, !choices.is_empty())),
        file_types: None,
        choices: choices
            .iter()
            .map(|label| CommandParameterChoice {
                name: Cow::Owned(label.clone()),
                localizations: Cow::Owned(Vec::new()),
                __non_exhaustive: (),
            })
            .collect(),
        channel_types: (!option.channel_types.is_empty())
            .then(|| Cow::Owned(option.channel_types.clone())),
        autocomplete_callback: None,
        __non_exhaustive: (),
    }
}

/// Resolves an option kind to its `CreateCommandOption` type setter. Choices
/// keep kind Integer without the min/max clamp (Discord rejects clamps on
/// choice-bearing options); see module docs for the deferred per-option
/// constraints (#114).
fn type_setter_for(
    kind: OptionKind,
    has_choices: bool,
) -> fn(
    poise::serenity_prelude::CreateCommandOption<'_>,
) -> poise::serenity_prelude::CreateCommandOption<'_> {
    match (kind, has_choices) {
        (OptionKind::String, _) => set_string_type,
        (OptionKind::Integer, false) => set_integer_type,
        (OptionKind::Integer, true) => set_integer_choice_type,
        (OptionKind::Number, _) => set_number_type,
        (OptionKind::Boolean, _) => set_boolean_type,
        (OptionKind::User, _) => set_user_type,
        (OptionKind::Channel, _) => set_channel_type,
        (OptionKind::Role, _) => set_role_type,
        (OptionKind::Mentionable, _) => set_mentionable_type,
        (OptionKind::Attachment, _) => set_attachment_type,
        (OptionKind::SubCommand | OptionKind::SubCommandGroup, _) => {
            unreachable!("subcommand kinds never reach type_setter_for")
        }
    }
}

fn set_string_type(
    builder: poise::serenity_prelude::CreateCommandOption<'_>,
) -> poise::serenity_prelude::CreateCommandOption<'_> {
    builder.kind(poise::serenity_prelude::CommandOptionType::String)
}

fn set_integer_type(
    builder: poise::serenity_prelude::CreateCommandOption<'_>,
) -> poise::serenity_prelude::CreateCommandOption<'_> {
    use poise::serenity_prelude::CommandOptionType;
    builder
        .min_number_value(f64::max(i64::MIN as f64, -9007199254740991.))
        .max_number_value(f64::min(i64::MAX as f64, 9007199254740991.))
        .kind(CommandOptionType::Integer)
}

fn set_integer_choice_type(
    builder: poise::serenity_prelude::CreateCommandOption<'_>,
) -> poise::serenity_prelude::CreateCommandOption<'_> {
    builder.kind(poise::serenity_prelude::CommandOptionType::Integer)
}

fn set_number_type(
    builder: poise::serenity_prelude::CreateCommandOption<'_>,
) -> poise::serenity_prelude::CreateCommandOption<'_> {
    builder.kind(poise::serenity_prelude::CommandOptionType::Number)
}

fn set_boolean_type(
    builder: poise::serenity_prelude::CreateCommandOption<'_>,
) -> poise::serenity_prelude::CreateCommandOption<'_> {
    builder.kind(poise::serenity_prelude::CommandOptionType::Boolean)
}

fn set_user_type(
    builder: poise::serenity_prelude::CreateCommandOption<'_>,
) -> poise::serenity_prelude::CreateCommandOption<'_> {
    builder.kind(poise::serenity_prelude::CommandOptionType::User)
}

fn set_channel_type(
    builder: poise::serenity_prelude::CreateCommandOption<'_>,
) -> poise::serenity_prelude::CreateCommandOption<'_> {
    builder.kind(poise::serenity_prelude::CommandOptionType::Channel)
}

fn set_role_type(
    builder: poise::serenity_prelude::CreateCommandOption<'_>,
) -> poise::serenity_prelude::CreateCommandOption<'_> {
    builder.kind(poise::serenity_prelude::CommandOptionType::Role)
}

fn set_mentionable_type(
    builder: poise::serenity_prelude::CreateCommandOption<'_>,
) -> poise::serenity_prelude::CreateCommandOption<'_> {
    builder.kind(poise::serenity_prelude::CommandOptionType::Mentionable)
}

fn set_attachment_type(
    builder: poise::serenity_prelude::CreateCommandOption<'_>,
) -> poise::serenity_prelude::CreateCommandOption<'_> {
    builder.kind(poise::serenity_prelude::CommandOptionType::Attachment)
}

/// Parses an `options` array into leaf/subcommand rows. Non-object rows and
/// rows missing a string `name`/`type` are skipped; an unknown `type` value
/// hard-errors with the offending row's index.
fn parse_options(value: Option<&Value>) -> Result<Vec<OptionSpec>, CommandSpecError> {
    let Some(Value::Array(rows)) = value else {
        return Ok(Vec::new());
    };
    let mut options = Vec::new();
    for (index, row) in rows.iter().enumerate() {
        match parse_option(row, index) {
            Ok(Some(option)) => options.push(option),
            Ok(None) => {} // malformed row: skipped
            Err(error) => return Err(error),
        }
    }
    Ok(options)
}

/// Parses one option row; `Ok(None)` skips malformed rows.
fn parse_option(value: &Value, index: usize) -> Result<Option<OptionSpec>, CommandSpecError> {
    let Some(object) = value.as_object() else {
        return Ok(None);
    };
    let Some(name) = object.get("name").and_then(Value::as_str) else {
        return Ok(None);
    };
    let Some(type_value) = object.get("type").and_then(Value::as_i64) else {
        return Ok(None);
    };
    let Some(kind) = OptionKind::from_discord(type_value) else {
        return Err(CommandSpecError::UnknownOptionType {
            index,
            name: name.to_string(),
            kind: type_value,
        });
    };
    let choices = parse_choice_labels(object.get("choices"));
    Ok(Some(OptionSpec {
        name: name.to_string(),
        description: object
            .get("description")
            .and_then(Value::as_str)
            .map(str::to_string),
        kind,
        required: bool_flag(object, "required"),
        choices,
        channel_types: parse_enum_list(object.get("channel_types"), map_channel_type)
            .unwrap_or_default(),
        min_value: object.get("min_value").and_then(Value::as_number).cloned(),
        max_value: object.get("max_value").and_then(Value::as_number).cloned(),
        min_length: parse_u16(object.get("min_length")),
        max_length: parse_u16(object.get("max_length")),
        autocomplete: bool_flag(object, "autocomplete"),
        options: parse_options(object.get("options"))?,
    }))
}

/// Parses choice labels from an array of `{"name": ...}` objects. The first
/// non-string or missing label ends the list: labels are positionally indexed
/// (`label = choices[i]`), so skipping a bad middle row would shift every
/// later index.
fn parse_choice_labels(value: Option<&Value>) -> Vec<String> {
    let Some(Value::Array(choices)) = value else {
        return Vec::new();
    };
    let mut labels = Vec::new();
    for choice in choices {
        let Some(name) = choice.get("name").and_then(Value::as_str) else {
            break;
        };
        labels.push(name.to_string());
    }
    labels
}

/// Parses an array of enum numbers through a mapper; anything else degrades to
/// `None` (callers pick the default). The `enum_number!` newtypes have no
/// `From<u8>`, hence the explicit `fn(u8) -> Option<T>`.
fn parse_enum_list<T: Copy>(value: Option<&Value>, map: fn(u8) -> Option<T>) -> Option<Vec<T>> {
    let Value::Array(values) = value? else {
        return None;
    };
    let mut result = Vec::with_capacity(values.len());
    for value in values {
        let number = value.as_u64()?;
        result.push(map(number as u8)?);
    }
    Some(result)
}

fn map_installation_context(value: u8) -> Option<serenity::InstallationContext> {
    Some(match value {
        0 => serenity::InstallationContext::Guild,
        1 => serenity::InstallationContext::User,
        _ => return None,
    })
}

fn map_interaction_context(value: u8) -> Option<serenity::InteractionContext> {
    Some(match value {
        0 => serenity::InteractionContext::Guild,
        1 => serenity::InteractionContext::BotDm,
        2 => serenity::InteractionContext::PrivateChannel,
        _ => return None,
    })
}

fn map_channel_type(value: u8) -> Option<serenity::ChannelType> {
    Some(match value {
        0 => serenity::ChannelType::Text,
        1 => serenity::ChannelType::Private,
        2 => serenity::ChannelType::Voice,
        3 => serenity::ChannelType::GroupDm,
        4 => serenity::ChannelType::Category,
        5 => serenity::ChannelType::News,
        10 => serenity::ChannelType::NewsThread,
        11 => serenity::ChannelType::PublicThread,
        12 => serenity::ChannelType::PrivateThread,
        13 => serenity::ChannelType::Stage,
        14 => serenity::ChannelType::Directory,
        15 => serenity::ChannelType::Forum,
        _ => return None,
    })
}

/// Parses a JSON number into a `u16`; anything else degrades to `None`.
fn parse_u16(value: Option<&Value>) -> Option<u16> {
    value
        .and_then(Value::as_u64)
        .and_then(|number| u16::try_from(number).ok())
}

/// Reads a boolean flag from an object; anything but `true`/`false` is
/// treated as absent.
fn bool_flag(object: &serde_json::Map<String, Value>, key: &str) -> bool {
    matches!(object.get(key), Some(Value::Bool(true)))
}

#[cfg(test)]
mod tests {
    use pwr_plugin_protocol::CommandDef;
    use pwr_plugin_protocol::Manifest;
    use serde_json::json;

    use super::*;

    /// Builds a `CommandData` from a partial interaction payload, the same way
    /// serenity does when an interaction arrives; `.options()` on it yields the
    /// resolved interaction arguments. Goes through a JSON string, not
    /// `from_value`: resolved channels deserialize via `&RawValue`, which only
    /// serde_json's string deserializer supports.
    fn command_data(payload: Value) -> serenity::CommandData {
        serde_json::from_str(&payload.to_string()).expect("command data deserializes")
    }

    /// Builds a `CommandInteraction` from a partial payload, the same way
    /// serenity does when an interaction arrives; `data` is the command data
    /// the interaction carries. Like [`command_data`], goes through a JSON
    /// string so resolved values deserialize via `&RawValue`.
    fn command_interaction(data: Value) -> serenity::CommandInteraction {
        serde_json::from_str(
            &json!({
                "id": "1",
                "application_id": "1",
                "channel_id": "1",
                "channel": {
                    "type": 1,
                    "id": "1",
                    "name": null,
                    "last_message_id": null,
                    "last_pin_timestamp": null,
                    "rate_limit_per_user": null,
                    "permissions": null,
                    "app_permissions": null,
                    "topic": null,
                },
                "token": "token",
                "version": 1,
                "app_permissions": "0",
                "locale": "en-US",
                "entitlements": [],
                "attachment_size_limit": 0,
                "data": data,
            })
            .to_string(),
        )
        .expect("command interaction deserializes")
    }

    /// Builds a leaf `OptionSpec` for the given kind.
    fn param(name: &str, kind: OptionKind) -> OptionSpec {
        OptionSpec {
            name: name.to_string(),
            description: Some(name.to_string()),
            kind,
            required: false,
            choices: Vec::new(),
            channel_types: Vec::new(),
            min_value: None,
            max_value: None,
            min_length: None,
            max_length: None,
            autocomplete: false,
            options: Vec::new(),
        }
    }

    /// Builds a manifest carrying the given command blobs.
    fn manifest(create_commands: Vec<Value>) -> Manifest {
        let mut manifest = crate::test_helpers::manifest_named("test-plugin");
        manifest.commands = create_commands
            .into_iter()
            .map(|create_command| CommandDef { create_command })
            .collect();
        manifest
    }

    // ── HostCommandSpec::parse ──────────────────────────────────────────────

    #[test]
    fn minimal_blob_parses_to_defaults() {
        let spec = HostCommandSpec::parse(&json!({
            "name": "ping",
            "description": "Pong!",
        }))
        .unwrap();

        assert_eq!(spec.name, "ping");
        assert_eq!(spec.description, "Pong!");
        assert!(spec.options.is_empty());
        assert_eq!(spec.default_member_permissions, None);
        assert!(!spec.dm_only);
        assert!(!spec.nsfw_only);
        assert!(!spec.guild_only);
        assert!(!spec.ephemeral);
        assert_eq!(spec.install_context, None);
        assert_eq!(spec.interaction_context, None);
    }

    #[test]
    fn full_flags_and_options_parse() {
        let spec = HostCommandSpec::parse(&json!({
            "name": "tune",
            "description": "Tune the radio",
            "nsfw": true,
            "dm_only": true,
            "guild_only": false,
            "ephemeral": true,
            "default_member_permissions": "2048",
            "options": [
                {"name": "text", "description": "Text", "type": 3,
                    "min_length": 2, "max_length": 20, "autocomplete": true},
                {"name": "count", "description": "Count", "type": 4,
                    "min_value": 1, "max_value": 10},
                {"name": "speed", "description": "Speed", "type": 10, "min_value": 0.5},
                {"name": "where", "description": "Where", "type": 7, "channel_types": [0, 2]},
                {"name": "note", "description": "Note", "type": 5, "choices": [
                    {"name": "yes", "value": 0},
                    {"name": "no", "value": 1},
                ]},
            ],
        }))
        .unwrap();

        assert_eq!(spec.name, "tune");
        assert!(spec.nsfw_only);
        assert!(spec.dm_only);
        assert!(!spec.guild_only);
        assert!(spec.ephemeral);
        assert_eq!(
            spec.default_member_permissions,
            Some(serenity::Permissions::from_bits_truncate(2048))
        );
        assert_eq!(spec.options.len(), 5);

        let text = &spec.options[0];
        assert_eq!(text.kind, OptionKind::String);
        assert_eq!(text.min_length, Some(2));
        assert_eq!(text.max_length, Some(20));
        assert!(text.autocomplete);

        let count = &spec.options[1];
        assert_eq!(count.kind, OptionKind::Integer);
        assert_eq!(count.min_value, Some(serde_json::Number::from(1_i64)));
        assert_eq!(count.max_value, Some(serde_json::Number::from(10_i64)));

        let speed = &spec.options[2];
        assert_eq!(speed.kind, OptionKind::Number);
        assert_eq!(
            speed.min_value,
            Some(serde_json::Number::from_f64(0.5).unwrap())
        );

        let where_ = &spec.options[3];
        assert_eq!(where_.kind, OptionKind::Channel);
        assert_eq!(
            where_.channel_types,
            vec![serenity::ChannelType::Text, serenity::ChannelType::Voice]
        );

        let note = &spec.options[4];
        assert_eq!(note.kind, OptionKind::Boolean);
        assert_eq!(note.choices, vec!["yes".to_string(), "no".to_string()]);
    }

    #[test]
    fn choice_labels_truncate_at_first_bad_label() {
        let spec = HostCommandSpec::parse(&json!({
            "name": "pick",
            "description": "Pick a level",
            "options": [
                {"name": "level", "description": "Level", "type": 3, "choices": [
                    {"name": "fast"},
                    {"value": 1},
                    {"name": "slow"},
                ]},
            ],
        }))
        .unwrap();

        // Labels are positionally indexed; a skipped middle row would shift
        // every later index, so parsing stops at the bad row.
        assert_eq!(spec.options[0].choices, vec!["fast".to_string()]);
    }

    #[test]
    fn contexts_and_integration_types_parse() {
        let spec = HostCommandSpec::parse(&json!({
            "name": "mod",
            "description": "Moderate",
            "integration_types": [0, 1],
            "contexts": [0, 1, 2],
        }))
        .unwrap();

        assert_eq!(
            spec.install_context,
            Some(vec![
                serenity::InstallationContext::Guild,
                serenity::InstallationContext::User,
            ])
        );
        assert_eq!(
            spec.interaction_context,
            Some(vec![
                serenity::InteractionContext::Guild,
                serenity::InteractionContext::BotDm,
                serenity::InteractionContext::PrivateChannel,
            ])
        );
    }

    #[test]
    fn dm_permission_false_maps_to_guild_context() {
        let spec = HostCommandSpec::parse(&json!({
            "name": "hidden",
            "description": "Guild only",
            "dm_permission": false,
        }))
        .unwrap();

        assert_eq!(
            spec.interaction_context,
            Some(vec![serenity::InteractionContext::Guild])
        );
    }

    #[test]
    fn explicit_contexts_override_dm_permission() {
        let spec = HostCommandSpec::parse(&json!({
            "name": "hybrid",
            "description": "Everywhere",
            "dm_permission": false,
            "contexts": [0, 2],
        }))
        .unwrap();

        assert_eq!(
            spec.interaction_context,
            Some(vec![
                serenity::InteractionContext::Guild,
                serenity::InteractionContext::PrivateChannel,
            ])
        );
    }

    #[test]
    fn garbage_inputs_degrade_to_defaults() {
        let spec = HostCommandSpec::parse(&json!({
            "name": "messy",
            "description": "Messy",
            "nsfw": "yes",
            "dm_only": 1,
            "ephemeral": null,
            "default_member_permissions": "not-a-number",
            "contexts": "all",
            "integration_types": [7],
            "options": "nope",
        }))
        .unwrap();

        assert!(!spec.nsfw_only);
        assert!(!spec.dm_only);
        assert!(!spec.ephemeral);
        assert_eq!(spec.default_member_permissions, None);
        assert_eq!(spec.install_context, None);
        assert_eq!(spec.interaction_context, None);
        assert!(spec.options.is_empty());
    }

    #[test]
    fn malformed_option_rows_are_skipped() {
        let spec = HostCommandSpec::parse(&json!({
            "name": "skip",
            "description": "Skip bad rows",
            "options": [
                {"name": "good", "description": "Fine", "type": 3},
                "not-an-object",
                {"description": "No name"},
                {"name": "no-type", "description": "Missing type"},
            ],
        }))
        .unwrap();

        assert_eq!(spec.options.len(), 1);
        assert_eq!(spec.options[0].name, "good");
    }

    #[test]
    fn unknown_option_type_is_rejected() {
        let error = HostCommandSpec::parse(&json!({
            "name": "bad",
            "description": "Bad option",
            "options": [
                {"name": "odd", "description": "Weird", "type": 99},
            ],
        }))
        .unwrap_err();

        assert_eq!(
            error,
            CommandSpecError::UnknownOptionType {
                index: 0,
                name: "odd".to_string(),
                kind: 99,
            }
        );
    }

    #[test]
    fn missing_required_blob_fields_error() {
        assert!(matches!(
            HostCommandSpec::parse(&json!("not-an-object")),
            Err(CommandSpecError::NotAnObject)
        ));
        assert!(matches!(
            HostCommandSpec::parse(&json!({"description": "no name"})),
            Err(CommandSpecError::MissingName)
        ));
        assert!(matches!(
            HostCommandSpec::parse(&json!({"name": "no description"})),
            Err(CommandSpecError::MissingDescription)
        ));
    }

    // ── command_from_blob ───────────────────────────────────────────────────

    #[test]
    fn routing_command_builds_with_flags() {
        let command = command_from_blob(&json!({
            "name": "tune",
            "description": "Tune the radio",
            "nsfw": true,
            "guild_only": true,
            "ephemeral": true,
            "dm_only": false,
            "default_member_permissions": "2048",
            "options": [
                {"name": "station", "description": "Callsign", "type": 3, "required": true},
            ],
        }))
        .unwrap();

        assert!(command.nsfw_only);
        assert!(command.guild_only);
        assert!(command.ephemeral);
        assert!(!command.dm_only);
        assert_eq!(
            command.default_member_permissions,
            serenity::Permissions::from_bits_truncate(2048)
        );
        assert!(command.subcommands.is_empty());
        assert!(!command.subcommand_required);
        assert_eq!(command.parameters.len(), 1);
        assert!(command.parameters[0].required);
        assert_eq!(command.parameters[0].name, "station");
    }

    #[test]
    fn parameter_type_kinds_map_to_discord_types() {
        let command = command_from_blob(&json!({
            "name": "kinds",
            "description": "All leaf kinds",
            "options": [
                {"name": "s", "description": "S", "type": 3},
                {"name": "i", "description": "I", "type": 4},
                {"name": "n", "description": "N", "type": 10},
                {"name": "b", "description": "B", "type": 5},
                {"name": "u", "description": "U", "type": 6},
                {"name": "c", "description": "C", "type": 7},
                {"name": "r", "description": "R", "type": 8},
                {"name": "m", "description": "M", "type": 9},
                {"name": "a", "description": "A", "type": 11},
            ],
        }))
        .unwrap();

        let payload = serde_json::to_value(command.create_as_slash_command().unwrap()).unwrap();
        let types: Vec<u8> = payload["options"]
            .as_array()
            .unwrap()
            .iter()
            .map(|option| option["type"].as_u64().unwrap() as u8)
            .collect();
        assert_eq!(types, [3, 4, 10, 5, 6, 7, 8, 9, 11]);
    }

    #[test]
    fn choices_register_as_integer_indices() {
        let command = command_from_blob(&json!({
            "name": "speed",
            "description": "Pick a speed",
            "options": [
                {"name": "level", "description": "Level", "type": 4, "choices": [
                    {"name": "fast", "value": 0},
                    {"name": "slow", "value": 1},
                ]},
            ],
        }))
        .unwrap();

        let payload = serde_json::to_value(command.create_as_slash_command().unwrap()).unwrap();
        let choices = payload["options"][0]["choices"].as_array().unwrap();
        assert_eq!(choices.len(), 2);
        assert_eq!(choices[0]["name"], "fast");
        assert_eq!(choices[0]["value"], 0);
        assert_eq!(choices[1]["name"], "slow");
        assert_eq!(choices[1]["value"], 1);
    }

    #[test]
    fn non_choice_kind_drops_choices_at_build_and_reparses_plain() {
        let command = command_from_blob(&json!({
            "name": "flag",
            "description": "A boolean flag",
            "options": [
                {"name": "on", "description": "On", "type": 5, "choices": [
                    {"name": "yes", "value": 0},
                    {"name": "no", "value": 1},
                ]},
            ],
        }))
        .unwrap();

        // Discord rejects choices on booleans, so registration carries none.
        // Serenity serializes an empty choices array (no skip_serializing_if on
        // the #[serde(default)] field), so assert emptiness rather than absence.
        let payload = serde_json::to_value(command.create_as_slash_command().unwrap()).unwrap();
        assert_eq!(payload["options"][0]["type"], 5);
        assert_eq!(payload["options"][0]["choices"], json!([]));

        // Re-parse matches: the same kind-with-choices spec resolves as a
        // plain boolean, ignoring the choices list.
        let mut spec = param("on", OptionKind::Boolean);
        spec.choices = vec!["yes".to_string(), "no".to_string()];
        let data = command_data(json!({
            "id": "1",
            "name": "cmd",
            "type": 1,
            "options": [{"name": "on", "type": 5, "value": true}],
        }));
        let args = data.options();
        let values = reparse_args(&[spec], &args).expect("boolean resolves");
        assert_eq!(values.get("on"), Some(&ArgValue::Boolean(true)));
    }

    #[test]
    fn subcommand_tree_builds_with_required_parents() {
        let command = command_from_blob(&json!({
            "name": "settings",
            "description": "Settings",
            "options": [
                {"name": "feed", "description": "Feed settings", "type": 1, "options": [
                    {"name": "list", "description": "List feeds", "type": 1},
                    {"name": "add", "description": "Add a feed", "type": 1, "options": [
                        {"name": "url", "description": "Feed URL", "type": 3, "required": true},
                    ]},
                ]},
                {"name": "dropped", "description": "Leaf after subcommand", "type": 3},
            ],
        }))
        .unwrap();

        assert!(command.subcommand_required);
        assert!(command.parameters.is_empty());
        assert_eq!(command.subcommands.len(), 1);

        let feed = &command.subcommands[0];
        assert_eq!(feed.name, "feed");
        assert!(feed.subcommand_required);
        assert!(feed.parameters.is_empty());
        assert_eq!(feed.subcommands.len(), 2);

        let list = &feed.subcommands[0];
        assert_eq!(list.name, "list");
        assert!(!list.subcommand_required);

        let add = &feed.subcommands[1];
        assert_eq!(add.name, "add");
        assert!(!add.subcommand_required);
        assert_eq!(add.parameters.len(), 1);
        assert!(add.parameters[0].required);
        assert_eq!(add.parameters[0].name, "url");
    }

    #[test]
    fn mixed_subcommand_node_drops_leaf_parameters() {
        let command = command_from_blob(&json!({
            "name": "run",
            "description": "Run a job",
            "options": [
                {"name": "job", "description": "Job", "type": 1, "options": [
                    {"name": "fast", "description": "Fast mode", "type": 1},
                    {"name": "mode", "description": "Mode", "type": 3},
                ]},
            ],
        }))
        .unwrap();

        // A node carries parameters or subcommands, never both: the leaf row
        // next to a subcommand row is dropped, mirroring the top level.
        let job = &command.subcommands[0];
        assert!(job.subcommand_required);
        assert!(job.parameters.is_empty());
        assert_eq!(job.subcommands.len(), 1);
        assert_eq!(job.subcommands[0].name, "fast");
    }

    #[test]
    fn registration_payload_carries_discord_flags() {
        let command = command_from_blob(&json!({
            "name": "mod",
            "description": "Moderate",
            "nsfw": true,
            "guild_only": true,
            "default_member_permissions": "2048",
            "options": [
                {"name": "target", "description": "Target", "type": 6, "required": true},
            ],
        }))
        .unwrap();

        assert!(command.nsfw_only);
        let payload = serde_json::to_value(command.create_as_slash_command().unwrap()).unwrap();
        assert_eq!(payload["name"], "mod");
        assert_eq!(payload["description"], "Moderate");
        assert_eq!(payload["default_member_permissions"], "2048");
        assert_eq!(payload["contexts"], json!([0]));
        assert_eq!(payload["nsfw"], false); // poise does not forward nsfw_only
        assert_eq!(payload["options"][0]["name"], "target");
        assert_eq!(payload["options"][0]["type"], 6);
    }

    #[test]
    fn command_from_blob_rejects_missing_fields() {
        assert!(matches!(
            command_from_blob(&json!("not-an-object")),
            Err(CommandSpecError::NotAnObject)
        ));
        assert!(matches!(
            command_from_blob(&json!({"description": "no name"})),
            Err(CommandSpecError::MissingName)
        ));
        assert!(matches!(
            command_from_blob(&json!({"name": "no description"})),
            Err(CommandSpecError::MissingDescription)
        ));
    }

    // ── reparse_args ────────────────────────────────────────────────────────

    #[test]
    fn reparse_extracts_scalar_args() {
        let specs = [
            param("user", OptionKind::String),
            param("count", OptionKind::Integer),
            param("ratio", OptionKind::Number),
            param("loud", OptionKind::Boolean),
        ];
        let data = command_data(json!({
            "id": "1",
            "name": "cmd",
            "type": 1,
            "options": [
                {"name": "user", "type": 3, "value": "alice"},
                {"name": "count", "type": 4, "value": 42},
                {"name": "ratio", "type": 10, "value": 0.5},
                {"name": "loud", "type": 5, "value": true},
            ],
        }));
        let args = data.options();
        let values = reparse_args(&specs, &args).expect("all specs matched");
        assert_eq!(
            values.get("user"),
            Some(&ArgValue::String("alice".to_string()))
        );
        assert_eq!(values.get("count"), Some(&ArgValue::Integer(42)));
        assert_eq!(values.get("ratio"), Some(&ArgValue::Number(0.5)));
        assert_eq!(values.get("loud"), Some(&ArgValue::Boolean(true)));
    }

    #[test]
    fn reparse_resolves_channel_and_choice() {
        let mut speed = param("speed", OptionKind::Integer);
        speed.required = true;
        speed.choices = vec!["slow".to_string(), "fast".to_string()];

        let specs = [param("where", OptionKind::Channel), speed];
        let data = command_data(json!({
            "id": "1",
            "name": "cmd",
            "type": 1,
            "options": [
                {"name": "where", "type": 7, "value": "123456789012345678"},
                {"name": "speed", "type": 4, "value": 0},
            ],
            "resolved": {
                "channels": {
                    "123456789012345678": {
                        "name": "general",
                        "type": 0,
                        "id": "123456789012345678",
                    },
                },
            },
        }));
        let args = data.options();
        let values = reparse_args(&specs, &args).expect("channel and choice resolve");
        assert_eq!(
            values.get("where"),
            Some(&ArgValue::ChannelId(123_456_789_012_345_678))
        );
        assert_eq!(
            values.get("speed"),
            Some(&ArgValue::Choice("slow".to_string()))
        );
    }

    #[test]
    fn string_and_number_choices_resolve_to_labels() {
        let mut level = param("level", OptionKind::String);
        level.choices = vec!["fast".to_string(), "slow".to_string()];
        let mut ratio = param("ratio", OptionKind::Number);
        ratio.choices = vec!["half".to_string(), "full".to_string()];

        let specs = [level, ratio];
        let data = command_data(json!({
            "id": "1",
            "name": "cmd",
            "type": 1,
            "options": [
                {"name": "level", "type": 3, "value": "fast"},
                {"name": "ratio", "type": 10, "value": 1},
            ],
        }));
        let args = data.options();
        let values = reparse_args(&specs, &args).expect("choices resolve");

        // String transports the label as its value; Number transports it as
        // the position index.
        assert_eq!(
            values.get("level"),
            Some(&ArgValue::Choice("fast".to_string()))
        );
        assert_eq!(
            values.get("ratio"),
            Some(&ArgValue::Choice("full".to_string()))
        );
    }

    #[test]
    fn reparse_reports_missing_required() {
        let mut spec = param("url", OptionKind::String);
        spec.required = true;
        let error = reparse_args(&[spec], &[]).expect_err("required arg absent");
        assert_eq!(
            error,
            ReparseError::MissingRequired {
                name: "url".to_string()
            }
        );
    }

    #[test]
    fn reparse_ignores_unknown_args_and_optional_absent() {
        let specs = [param("url", OptionKind::String)];
        let data = command_data(json!({
            "id": "1",
            "name": "cmd",
            "type": 1,
            "options": [{"name": "other", "type": 4, "value": 1}],
        }));
        let args = data.options();
        let values = reparse_args(&specs, &args).expect("no required args");
        assert!(values.is_empty());
    }

    #[test]
    fn reparse_reports_type_mismatch() {
        let specs = [param("count", OptionKind::Integer)];
        let data = command_data(json!({
            "id": "1",
            "name": "cmd",
            "type": 1,
            "options": [{"name": "count", "type": 3, "value": "three"}],
        }));
        let args = data.options();
        let error = reparse_args(&specs, &args).expect_err("wrong value type");
        assert_eq!(
            error,
            ReparseError::TypeMismatch {
                name: "count".to_string(),
                expected: "integer",
                found: "string",
            }
        );
    }

    #[test]
    fn reparse_reports_out_of_range_choice_index() {
        let mut spec = param("speed", OptionKind::Integer);
        spec.choices = vec!["slow".to_string()];
        let data = command_data(json!({
            "id": "1",
            "name": "cmd",
            "type": 1,
            "options": [{"name": "speed", "type": 4, "value": 7}],
        }));
        let args = data.options();
        let error = reparse_args(&[spec], &args).expect_err("choice index 7 of 1");
        assert_eq!(
            error,
            ReparseError::ChoiceIndexOutOfRange {
                name: "speed".to_string(),
                index: 7,
                count: 1,
            }
        );
    }

    // ── leaf_options ────────────────────────────────────────────────────────

    #[test]
    fn leaf_options_descend_through_subcommand_groups() {
        let data = command_data(json!({
            "id": "1",
            "name": "cmd",
            "type": 1,
            "options": [{
                "name": "settings",
                "type": 2,
                "options": [{
                    "name": "feed",
                    "type": 1,
                    "options": [{"name": "url", "type": 3, "value": "https://example.com"}],
                }],
            }],
        }));
        let args = data.options();

        let leaves = leaf_options(&args);
        assert_eq!(leaves.len(), 1);
        assert_eq!(leaves[0].name, "url");
        if let serenity::ResolvedValue::String(url) = &leaves[0].value {
            assert_eq!(*url, "https://example.com");
        } else {
            panic!("expected a string leaf");
        }
    }

    #[test]
    fn leaf_options_passthrough_without_subcommands() {
        let data = command_data(json!({
            "id": "1",
            "name": "cmd",
            "type": 1,
            "options": [
                {"name": "url", "type": 3, "value": "https://example.com"},
                {"name": "count", "type": 4, "value": 3},
            ],
        }));
        let args = data.options();

        let leaves = leaf_options(&args);
        assert_eq!(leaves.len(), 2);
        assert_eq!(leaves[0].name, "url");
        assert_eq!(leaves[1].name, "count");
    }

    // ── dispatch re-parse (reparse_command_args) ────────────────────────────

    #[test]
    fn dispatch_reparses_scalar_args_into_the_plugin_payload() {
        let command = command_from_blob(&json!({
            "name": "settings",
            "description": "Settings",
            "options": [
                {"name": "channel", "description": "Channel", "type": 7, "required": true},
                {"name": "note", "description": "Note", "type": 3},
                {"name": "count", "description": "Count", "type": 4},
            ],
        }))
        .unwrap();
        let interaction = command_interaction(json!({
            "id": "1",
            "name": "settings",
            "type": 1,
            "options": [
                {"name": "channel", "type": 7, "value": "123456789012345678"},
                {"name": "note", "type": 3, "value": "hello"},
                {"name": "count", "type": 4, "value": 42},
            ],
            "resolved": {
                "channels": {
                    "123456789012345678": {
                        "name": "general",
                        "type": 0,
                        "id": "123456789012345678",
                    },
                },
            },
        }));

        let args = reparse_command_args(&command, &interaction).expect("args reparse");
        assert_eq!(
            args,
            json!({
                "channel": 123456789012345678_u64,
                "note": "hello",
                "count": 42,
            })
        );
    }

    #[test]
    fn dispatch_descends_subcommands_before_reparsing() {
        let command = command_from_blob(&json!({
            "name": "settings",
            "description": "Settings",
            "options": [
                {"name": "feed", "description": "Feed settings", "type": 1, "options": [
                    {"name": "add", "description": "Add a feed", "type": 1, "options": [
                        {"name": "url", "description": "Feed URL", "type": 3, "required": true},
                    ]},
                ]},
            ],
        }))
        .unwrap();
        let add = &command.subcommands[0].subcommands[0];
        let interaction = command_interaction(json!({
            "id": "1",
            "name": "settings",
            "type": 1,
            "options": [{
                "name": "feed",
                "type": 2,
                "options": [{
                    "name": "add",
                    "type": 1,
                    "options": [{"name": "url", "type": 3, "value": "https://example.com"}],
                }],
            }],
        }));

        let args = reparse_command_args(add, &interaction).expect("leaf args reparse");
        assert_eq!(args, json!({"url": "https://example.com"}));
    }

    #[test]
    fn dispatch_resolves_choices_to_labels() {
        let command = command_from_blob(&json!({
            "name": "speed",
            "description": "Pick a speed",
            "options": [
                {"name": "level", "description": "Level", "type": 4, "required": true, "choices": [
                    {"name": "fast", "value": 0},
                    {"name": "slow", "value": 1},
                ]},
            ],
        }))
        .unwrap();
        let interaction = command_interaction(json!({
            "id": "1",
            "name": "speed",
            "type": 1,
            "options": [{"name": "level", "type": 4, "value": 1}],
        }));

        let args = reparse_command_args(&command, &interaction).expect("choice reparse");
        assert_eq!(args, json!({"level": "slow"}));
    }

    #[test]
    fn dispatch_reports_a_missing_required_argument() {
        let command = command_from_blob(&json!({
            "name": "run",
            "description": "Run a job",
            "options": [
                {"name": "url", "description": "URL", "type": 3, "required": true},
            ],
        }))
        .unwrap();
        let interaction = command_interaction(json!({"id": "1", "name": "run", "type": 1}));

        let error = reparse_command_args(&command, &interaction).expect_err("required arg absent");
        assert_eq!(
            error,
            ReparseError::MissingRequired {
                name: "url".to_string()
            }
        );
    }

    #[test]
    fn dispatch_without_parameters_yields_an_empty_payload() {
        let command = command_from_blob(&json!({
            "name": "settings",
            "description": "Manage server settings",
        }))
        .unwrap();
        let interaction = command_interaction(json!({"id": "1", "name": "settings", "type": 1}));

        let args = reparse_command_args(&command, &interaction).expect("no specs to reparse");
        assert_eq!(args, json!({}));
    }

    #[test]
    fn foreign_commands_without_specs_yield_an_empty_payload() {
        let command = poise::Command::<Data, Error>::default();
        let interaction = command_interaction(json!({"id": "1", "name": "x", "type": 1}));

        let args = reparse_command_args(&command, &interaction).expect("downcast failure degrades");
        assert_eq!(args, json!({}));
    }

    // ── commands_from_manifest / select_commands ────────────────────────────

    #[test]
    fn commands_from_manifest_skips_invalid_blobs() {
        let commands = commands_from_manifest(&manifest(vec![
            json!({"name": "ok", "description": "Fine"}),
            json!({"name": "broken"}), // MissingDescription
            json!("not-an-object"),
        ]));

        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].name, "ok");
    }

    #[test]
    fn select_commands_filters_by_name_preserving_order() {
        let commands = commands_from_manifest(&manifest(vec![
            json!({"name": "alpha", "description": "A"}),
            json!({"name": "beta", "description": "B"}),
            json!({"name": "gamma", "description": "G"}),
        ]));

        let selected = select_commands(&commands, &["gamma", "alpha"]);
        let names: Vec<&str> = selected.iter().map(|c| c.name.as_ref()).collect();
        assert_eq!(names, ["alpha", "gamma"]); // order of `commands`, not `names`

        assert!(select_commands(&commands, &[]).is_empty());
    }

    // ── routes_from_manifests ───────────────────────────────────────────────

    #[test]
    fn routes_from_manifests_maps_command_names_to_their_plugins() {
        let routes = routes_from_manifests([
            (
                "settings".to_string(),
                Some(manifest(vec![
                    json!({"name": "settings", "description": "S"}),
                ])),
            ),
            ("hello".to_string(), None),
        ]);

        assert_eq!(
            routes,
            PluginRoutes::from([("settings".to_string(), "settings".to_string())])
        );
    }

    #[test]
    fn routes_from_manifests_skips_blobs_without_a_name() {
        let routes = routes_from_manifests([(
            "hello".to_string(),
            Some(manifest(vec![json!({"description": "no name"})])),
        )]);

        assert!(routes.is_empty());
    }
}
