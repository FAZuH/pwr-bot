# Discord Setup

Create a Discord application, invite the bot to your server, and register
its slash commands. You do this once per application. The bot reads the
values below from the environment file. See
[Configuration](configuration.md) for the variable names.

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

After the bot runs and joins your server, register the slash commands:

1. In a channel the bot can see, type `!register_owner`.
2. The bot responds with buttons.
3. Click **Register in guild** (immediate) or **Register globally** (can take up to one hour).

> [!note]
> The `!register_owner` command needs your Discord user ID to match `ADMIN_ID` in the environment file.
>
> Users in other servers with the "Administrator" or "Manage Server" permission can run `!register` or `!unregister`.

<img width="617" height="91" alt="image" src="https://github.com/user-attachments/assets/c0f508aa-e373-4df7-a574-01183eee4a98" />

## Related

- [README](../README.md) — installation and usage
- [Configuration](configuration.md) — `DISCORD_TOKEN`, `ADMIN_ID`, and `ENABLE_AUTOREGISTER_CMD`
