//! The voice plugin process: storage, event tracking, commands, and views.

use std::io::BufRead;
use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;

use chrono::Utc;
use pwr_plugin_protocol::API_VERSION;
use pwr_plugin_protocol::Msg;
use pwr_plugin_support::about_exit;
use pwr_plugin_support::back_exit;
use pwr_plugin_support::reply_err;
use pwr_plugin_support::write_msg;
use serde_json::Value;
use serde_json::json;
use voice::COMMAND_NAME;
use voice::PLUGIN_NAME;
use voice::VOICE_LEADERBOARD_COMMAND_NAME;
use voice::VOICE_SETTINGS_COMMAND_NAME;
use voice::VOICE_STATS_COMMAND_NAME;
use voice::command;
use voice::heartbeat::VoiceHeartbeatManager;
use voice::host_client::HostClient;
use voice::host_client::HostResponse;
use voice::host_client::SharedOutput;
use voice::host_client::spawn_host_client;
use voice::manifest;
use voice::repo::Repository;
use voice::service::VoiceTrackingService;
use voice::subscriber::VoiceStateEvent;
use voice::subscriber::VoiceStateSubscriber;

struct OutputWriter(SharedOutput);

impl Write for OutputWriter {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        self.0.lock().expect("stdout lock poisoned").write(buffer)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.0.lock().expect("stdout lock poisoned").flush()
    }
}

fn write_output(output: &SharedOutput, message: &Msg) -> std::io::Result<()> {
    let mut output = output.lock().expect("stdout lock poisoned");
    write_msg(&mut *output, message)
}

fn reply_spec(output: &SharedOutput, id: u64, spec: pwr_plugin_protocol::ViewSpec) {
    let data = serde_json::to_value(spec).expect("voice view serializes");
    let _ = write_output(output, &Msg::resp_ok(id, Some(data)));
}

fn reply_wire_error(output: &SharedOutput, id: u64, kind: &str, message: impl Into<String>) {
    let mut writer = OutputWriter(output.clone());
    let _ = reply_err(&mut writer, id, kind, message);
}

fn reply_error(output: &SharedOutput, id: u64, error: command::CommandError) {
    reply_wire_error(output, id, "CommandError", error.to_string());
}

async fn load_host_config(
    output: &SharedOutput,
    next_call_id: &Arc<AtomicU64>,
    input: &mut impl BufRead,
) -> Result<(String, PathBuf, Vec<Msg>), String> {
    let call_id = next_call_id.fetch_add(1, Ordering::Relaxed);
    write_output(
        output,
        &Msg::Call {
            id: call_id,
            op: "host.get_config".into(),
            cmd: None,
            args: None,
        },
    )
    .map_err(|error| error.to_string())?;
    let mut queued = Vec::new();
    for line in input.lines() {
        let line = line.map_err(|error| error.to_string())?;
        let message: Msg = serde_json::from_str(&line).map_err(|error| error.to_string())?;
        if matches!(&message, Msg::Hello { .. }) {
            continue;
        }
        if let Msg::Resp {
            id,
            ok: true,
            data: Some(data),
            ..
        } = &message
            && *id == call_id
        {
            let db_url = data
                .get("db_url")
                .and_then(Value::as_str)
                .ok_or("host.get_config response has no db_url")?
                .to_string();
            let data_path = data
                .get("data_path")
                .and_then(Value::as_str)
                .ok_or("host.get_config response has no data_path")?
                .into();
            return Ok((db_url, data_path, queued));
        }
        queued.push(message);
    }
    Err("host closed before returning config".into())
}

enum DispatchResult {
    View(pwr_plugin_protocol::ViewSpec),
    Moved,
}

async fn dispatch_call(
    service: Arc<VoiceTrackingService>,
    host: Arc<dyn HostClient>,
    op: &str,
    command_name: Option<&str>,
    args: Value,
) -> Result<DispatchResult, command::CommandError> {
    match (op, command_name) {
        ("invoke", Some(COMMAND_NAME)) | ("invoke", Some(VOICE_SETTINGS_COMMAND_NAME)) => {
            let actor = command::ActorContext::from_args(&args)?;
            actor.require_admin()?;
            let guild_id = actor.guild_id.ok_or(command::CommandError::GuildOnly)?;
            let settings = service
                .get_settings(guild_id)
                .await
                .map_err(|error| command::CommandError::Service(error.to_string()))?;
            Ok(DispatchResult::View(command::settings_spec(
                &command::SettingsSession { guild_id, settings },
            )))
        }
        ("invoke", Some(VOICE_STATS_COMMAND_NAME)) => {
            let session = command::invoke_stats(&service, &host, &args).await?;
            Ok(DispatchResult::View(command::stats_spec(&session)))
        }
        ("invoke", Some(VOICE_LEADERBOARD_COMMAND_NAME)) => {
            let session = command::invoke_leaderboard(&service, &host, &args).await?;
            Ok(DispatchResult::View(command::leaderboard_spec(&session)))
        }
        ("view.interact", Some(COMMAND_NAME))
        | ("view.interact", Some(VOICE_SETTINGS_COMMAND_NAME)) => {
            interact_settings(&service, &host, &args).await
        }
        ("view.interact", Some(VOICE_STATS_COMMAND_NAME)) => {
            let session = command::interact_stats(&service, &host, &args).await?;
            Ok(DispatchResult::View(command::stats_spec(&session)))
        }
        ("view.interact", Some(VOICE_LEADERBOARD_COMMAND_NAME)) => {
            let session = command::interact_leaderboard(&service, &host, &args).await?;
            Ok(DispatchResult::View(command::leaderboard_spec(&session)))
        }
        _ => Err(command::CommandError::InvalidArgument(format!(
            "unknown voice operation `{op}` for command `{}`",
            command_name.unwrap_or("")
        ))),
    }
}

async fn interact_settings(
    service: &Arc<VoiceTrackingService>,
    host: &Arc<dyn HostClient>,
    args: &Value,
) -> Result<DispatchResult, command::CommandError> {
    let actor = command::ActorContext::from_args(args)?;
    actor.require_admin()?;
    let mut session: command::SettingsSession =
        serde_json::from_value(args.get("view").cloned().ok_or_else(|| {
            command::CommandError::InvalidState("missing settings view state".into())
        })?)
        .map_err(|error| command::CommandError::InvalidState(error.to_string()))?;
    let custom_id = args
        .get("custom_id")
        .and_then(Value::as_str)
        .ok_or_else(|| command::CommandError::InvalidArgument("missing custom_id".into()))?;
    if custom_id == "voice:toggle" {
        session.settings.enabled = !session.settings.enabled;
        return Ok(DispatchResult::View(command::settings_spec(&session)));
    }
    if custom_id != "voice:back" && custom_id != "voice:about" {
        return Err(command::CommandError::InvalidArgument(format!(
            "unknown settings action `{custom_id}`"
        )));
    }
    service
        .update_settings(session.guild_id, session.settings.clone())
        .await
        .map_err(|error| command::CommandError::Service(error.to_string()))?;
    let exit = if custom_id == "voice:about" {
        about_exit(Some(args), session.guild_id)
    } else {
        back_exit(Some(args), session.guild_id)
    };
    let Some(exit) = exit else {
        return Ok(DispatchResult::View(command::settings_spec(&session)));
    };
    let response = host
        .call("host.open_view", exit.args.clone())
        .await
        .map_err(|error| command::CommandError::Service(error.to_string()))?;
    if exit.message_id.is_some() {
        let _ = response;
        return Ok(DispatchResult::Moved);
    }
    Ok(DispatchResult::View(command::settings_spec(&session)))
}

#[tokio::main]
async fn main() -> ExitCode {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let output: SharedOutput = Arc::new(Mutex::new(stdout));
    let next_call_id = Arc::new(AtomicU64::new(0));
    let mut input = stdin.lock();

    let hello = Msg::Hello {
        v: API_VERSION,
        name: PLUGIN_NAME.into(),
        ops: vec![
            "host.get_config".into(),
            "host.resolve_users".into(),
            "host.open_view".into(),
        ],
        manifest: Some(manifest()),
    };
    if write_output(&output, &hello).is_err() {
        return ExitCode::FAILURE;
    }
    let (db_url, data_path, queued_messages) =
        match load_host_config(&output, &next_call_id, &mut input).await {
            Ok(config) => config,
            Err(error) => {
                eprintln!("failed to load host config: {error}");
                return ExitCode::FAILURE;
            }
        };
    let repository = match Repository::connect(&db_url).await {
        Ok(repository) => repository,
        Err(error) => {
            eprintln!("failed to connect voice storage: {error}");
            return ExitCode::FAILURE;
        }
    };
    if let Err(error) = repository.migrate().await {
        eprintln!("failed to migrate voice storage: {error}");
        return ExitCode::FAILURE;
    }
    if let Err(error) = repository.import_legacy_settings_once().await {
        eprintln!("failed to import legacy voice settings: {error}");
        return ExitCode::FAILURE;
    }
    let service = match VoiceTrackingService::new(
        Arc::new(repository.voice_sessions.clone()),
        Arc::new(repository.voice_settings.clone()),
    )
    .await
    {
        Ok(service) => Arc::new(service),
        Err(error) => {
            eprintln!("failed to start voice service: {error}");
            return ExitCode::FAILURE;
        }
    };
    let heartbeat = Arc::new(VoiceHeartbeatManager::new(data_path, service.clone()));
    if let Err(error) = heartbeat.recover_from_crash().await {
        eprintln!("voice crash recovery failed: {error}");
        return ExitCode::FAILURE;
    }
    heartbeat.clone().start().await;
    let host = Arc::new(spawn_host_client(output.clone(), next_call_id.clone()));
    let host_dyn: Arc<dyn HostClient> = host.clone();
    let subscriber = Arc::new(VoiceStateSubscriber::new(service.clone()));
    let mut queued_messages = queued_messages.into_iter();
    let mut lines = input.lines();
    loop {
        let message = if let Some(message) = queued_messages.next() {
            message
        } else {
            let Some(Ok(line)) = lines.next() else { break };
            match serde_json::from_str(&line) {
                Ok(message) => message,
                Err(error) => {
                    eprintln!("bad json: {error}");
                    continue;
                }
            }
        };
        match message {
            Msg::Bye => break,
            Msg::Call { id, op, cmd, args } => {
                let service = service.clone();
                let host = host_dyn.clone();
                let output = output.clone();
                tokio::spawn(async move {
                    let args = args.unwrap_or_else(|| json!({}));
                    match dispatch_call(service, host, &op, cmd.as_deref(), args).await {
                        Ok(DispatchResult::View(spec)) => reply_spec(&output, id, spec),
                        Ok(DispatchResult::Moved) => {
                            reply_wire_error(&output, id, "ViewMoved", "view moved");
                        }
                        Err(error) => reply_error(&output, id, error),
                    }
                });
            }
            Msg::Event { name, data } => match name.as_str() {
                "voice_state" => {
                    if let Ok(event) =
                        serde_json::from_value::<VoiceStateEvent>(data.unwrap_or(Value::Null))
                        && let Err(error) = subscriber.callback(event).await
                    {
                        eprintln!("voice_state handling failed: {error}");
                    }
                }
                "guild_create" => {
                    if let Err(error) = subscriber
                        .callback_guild_create(data.as_ref().unwrap_or(&Value::Null))
                        .await
                    {
                        eprintln!("guild_create handling failed: {error}");
                    }
                }
                "view.timeout" => {
                    if let Some(data) = data {
                        if let Ok(session) =
                            serde_json::from_value::<command::SettingsSession>(data.clone())
                        {
                            if let Err(error) = service
                                .update_settings(session.guild_id, session.settings)
                                .await
                            {
                                eprintln!("voice settings timeout persistence failed: {error}");
                            }
                        } else if let Ok(mut session) =
                            serde_json::from_value::<command::LeaderboardSession>(data.clone())
                        {
                            let _ = voice::update::voice_leaderboard::update(
                                voice::update::voice_leaderboard::VoiceLeaderboardMsg::Lifecycle(
                                    pwr_plugin_support::lifecycle::Lifecycle::Expired,
                                ),
                                &mut session.model,
                            );
                        } else if let Ok(mut session) =
                            serde_json::from_value::<command::StatsSession>(data)
                        {
                            let _ = voice::update::voice_stats::update(
                                voice::update::voice_stats::VoiceStatsMsg::Lifecycle(
                                    pwr_plugin_support::lifecycle::Lifecycle::Expired,
                                ),
                                &mut session.model,
                                Utc::now(),
                            );
                        }
                    }
                }
                _ => {}
            },
            Msg::Ping => {
                let _ = write_output(&output, &Msg::Pong);
            }
            Msg::Pong | Msg::Progress { .. } | Msg::Hello { .. } => {}
            Msg::Resp {
                id,
                ok,
                data,
                error,
            } => {
                host.send_response(HostResponse {
                    id,
                    ok,
                    data,
                    error,
                });
            }
        }
    }
    heartbeat.update().await;
    ExitCode::SUCCESS
}
