# Discord Setup

Create a Discord application and invite the bot to your server. You do
this once per application. The bot reads the values below from the
environment file. See [Configuration](configuration.md) for the variable
names. To register slash commands after the bot runs, see
[Command Registration](../README.md#command-registration).

## Create the application

1. Go to the [Discord Developer Portal](https://discord.com/developers/applications).
2. Click **New Application** and give the application a name.
3. Open the **Bot** tab:
    - Click **Reset Token**. Save the token as `DISCORD_TOKEN`.
    - Under **Privileged Gateway Intents**, enable **Message Content Intent**.
4. Open **OAuth2 → URL Generator**:
    - Select the scopes `bot` and `applications.commands`.
    - Select these bot permissions: `View Channels`, `Send Messages`, `Embed Links`, and `Read Message History`. The `!register` command needs `Read Message History`.
5. Open the generated URL and invite the bot to your server.
6. Open the **General Information** tab and copy the **Application ID**. You need it for the `ENABLE_AUTOREGISTER_CMD` feature.

## Command Registration

After the bot runs and joins your server, register the slash commands.
See [Command Registration](../README.md#command-registration) in the README.

## Related

- [README](../README.md) — installation and usage
- [Configuration](configuration.md) — `DISCORD_TOKEN`, `ADMIN_ID`, and `ENABLE_AUTOREGISTER_CMD`
