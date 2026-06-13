//! FFI host context implementation.
//!
//! Implements [`HostCtx`] for the plugin FFI path by wrapping the host
//! callbacks and providing them to plugins through the `pwr_bot_sdk`
//! callback table.

use std::ffi::CStr;
use std::ffi::CString;
use std::sync::Arc;

use async_trait::async_trait;
use poise::serenity_prelude::*;

use bytes::BytesMut;
use postgres::types::IsNull;
use postgres::types::ToSql;
use postgres::types::Type;

/// A sentinel type representing a NULL database parameter.
#[derive(Debug)]
struct PgNull;
impl ToSql for PgNull {
    fn to_sql(&self, _ty: &Type, _out: &mut BytesMut) -> Result<IsNull, Box<dyn std::error::Error + Sync + Send>> {
        Ok(IsNull::Yes)
    }
    fn accepts(_ty: &Type) -> bool {
        true
    }
    fn to_sql_checked(&self, ty: &Type, out: &mut BytesMut) -> Result<IsNull, Box<dyn std::error::Error + Sync + Send>> {
        self.to_sql(ty, out)
    }
}

use crate::bot::Data;
use crate::bot::command::Error;
use crate::bot::host_ctx::HostCtx;
use crate::bot::host_ctx::MessagePayload;
use crate::bot::host_ctx::PoiseHostCtx;
use crate::bot::plugin::host_registry;

/// Global callback table shared by all FFI invocations.
static HOST_CALLBACKS: std::sync::OnceLock<pwr_bot_sdk::HostCallbacks> =
    std::sync::OnceLock::new();

/// Returns the global callback table.
pub fn host_callbacks() -> &'static pwr_bot_sdk::HostCallbacks {
    HOST_CALLBACKS.get_or_init(|| pwr_bot_sdk::HostCallbacks {
        send_reply: cb_send_reply,
        edit_reply: cb_edit_reply,
        defer: cb_defer,
        get_guild_id: cb_guild_id,
        get_author_id: cb_author_id,
        get_channel_id: cb_channel_id,
        free_string: cb_free_string,
        query_db: cb_query_db,
        execute_db: cb_execute_db,
        send_channel_message: cb_send_channel_message,
        send_dm: cb_send_dm,
        publish_event: cb_publish_event,
        get_poll_interval: cb_get_poll_interval,
        get_data_path: cb_get_data_path,
        is_feature_enabled: cb_is_feature_enabled,
    })
}

/// FFI-backed implementation of [`HostCtx`].
///
/// Each invocation gets a unique handle that plugins can use to call back
/// into the host. The host stores the actual [`PoiseHostCtx`] and dispatches
/// callback requests to it.
pub struct FfiHostCtx {
    handle: u64,
    inner: Arc<PoiseHostCtx>,
}

impl FfiHostCtx {
    pub fn new(inner: Arc<PoiseHostCtx>) -> Self {
        let handle = host_registry::register(inner.clone());
        Self { handle, inner }
    }

    pub fn handle(&self) -> u64 {
        self.handle
    }
}

impl Drop for FfiHostCtx {
    fn drop(&mut self) {
        host_registry::unregister(self.handle);
    }
}

// ---------------------------------------------------------------------------
// FFI callbacks
// ---------------------------------------------------------------------------

fn ctx_data(ctx_handle: u64) -> Option<Arc<Data>> {
    host_registry::get(ctx_handle).map(|ctx| ctx.data())
}

unsafe extern "C" fn cb_send_reply(
    ctx_handle: u64,
    reply_json: *const std::ffi::c_char,
    out_err: *mut *mut std::ffi::c_char,
) -> bool {
    let ctx = match host_registry::get(ctx_handle) {
        Some(ctx) => ctx,
        None => return false,
    };

    let json_str = match unsafe { CStr::from_ptr(reply_json) }.to_str() {
        Ok(s) => s,
        Err(_) => return false,
    };

    let payload: MessagePayload =
        match serde_json::from_str::<pwr_bot_sdk::ResponsePayload>(json_str) {
            Ok(resp) => MessagePayload {
                content: resp.content,
                embed: None,
                ephemeral: resp.ephemeral,
            },
            Err(e) => {
                let err = CString::new(format!("invalid reply JSON: {e}")).unwrap();
                if !out_err.is_null() {
                    unsafe { *out_err = err.into_raw() };
                }
                return false;
            }
        };

    let handle = tokio::runtime::Handle::current();
    match handle.block_on(async { ctx.send_message(&payload).await }) {
        Ok(_) => true,
        Err(e) => {
            let err = CString::new(e.to_string()).unwrap();
            if !out_err.is_null() {
                unsafe { *out_err = err.into_raw() };
            }
            false
        }
    }
}

unsafe extern "C" fn cb_edit_reply(
    ctx_handle: u64,
    message_id: u64,
    reply_json: *const std::ffi::c_char,
    out_err: *mut *mut std::ffi::c_char,
) -> bool {
    let ctx = match host_registry::get(ctx_handle) {
        Some(ctx) => ctx,
        None => return false,
    };

    let json_str = match unsafe { CStr::from_ptr(reply_json) }.to_str() {
        Ok(s) => s,
        Err(_) => return false,
    };

    let payload: MessagePayload =
        match serde_json::from_str::<pwr_bot_sdk::ResponsePayload>(json_str) {
            Ok(resp) => MessagePayload {
                content: resp.content,
                embed: None,
                ephemeral: resp.ephemeral,
            },
            Err(e) => {
                let err = CString::new(format!("invalid reply JSON: {e}")).unwrap();
                if !out_err.is_null() {
                    unsafe { *out_err = err.into_raw() };
                }
                return false;
            }
        };

    let handle = tokio::runtime::Handle::current();
    match handle.block_on(async { ctx.edit_message(MessageId::new(message_id), &payload).await }) {
        Ok(_) => true,
        Err(e) => {
            let err = CString::new(e.to_string()).unwrap();
            if !out_err.is_null() {
                unsafe { *out_err = err.into_raw() };
            }
            false
        }
    }
}

unsafe extern "C" fn cb_defer(ctx_handle: u64) -> bool {
    let ctx = match host_registry::get(ctx_handle) {
        Some(ctx) => ctx,
        None => return false,
    };

    let handle = tokio::runtime::Handle::current();
    handle.block_on(async { ctx.defer().await }).is_ok()
}

unsafe extern "C" fn cb_guild_id(ctx_handle: u64) -> u64 {
    host_registry::get(ctx_handle)
        .and_then(|ctx| ctx.guild_id())
        .unwrap_or(0)
}

unsafe extern "C" fn cb_author_id(ctx_handle: u64) -> u64 {
    host_registry::get(ctx_handle)
        .map(|ctx| ctx.author_id())
        .unwrap_or(0)
}

unsafe extern "C" fn cb_channel_id(ctx_handle: u64) -> u64 {
    host_registry::get(ctx_handle)
        .map(|ctx| ctx.channel_id())
        .unwrap_or(0)
}

unsafe extern "C" fn cb_free_string(s: *mut std::ffi::c_char) {
    if !s.is_null() {
        unsafe {
            let _ = CString::from_raw(s);
        }
    }
}

// ---- Database callbacks ----

fn deserialize_db_params(params_json: *const std::ffi::c_char) -> Vec<pwr_bot_sdk::DbValue> {
    if params_json.is_null() {
        return vec![];
    }
    match unsafe { CStr::from_ptr(params_json) }.to_str() {
        Ok(s) => serde_json::from_str(s).unwrap_or_default(),
        Err(_) => vec![],
    }
}

fn postgres_params_from_dbvalue(
    values: &[pwr_bot_sdk::DbValue],
) -> Vec<Box<dyn postgres::types::ToSql + Sync + '_>> {
    use postgres::types::ToSql;
    values
        .iter()
        .map(|v| -> Box<dyn ToSql + Sync + '_> {
            match v {
                pwr_bot_sdk::DbValue::Null => Box::new(PgNull),
                pwr_bot_sdk::DbValue::Bool(b) => Box::new(*b),
                pwr_bot_sdk::DbValue::I32(n) => Box::new(*n),
                pwr_bot_sdk::DbValue::I64(n) => Box::new(*n),
                pwr_bot_sdk::DbValue::F64(n) => Box::new(*n),
                pwr_bot_sdk::DbValue::Text(s) => Box::new(s.as_str()),
            }
        })
        .collect()
}

fn with_db<F, T>(ctx_handle: u64, f: F) -> Result<T, (String, bool)>
where
    F: FnOnce(&mut postgres::Client) -> Result<T, postgres::Error>,
{
    let data = ctx_data(ctx_handle).ok_or(("no context".to_string(), false))?;
    let db_url = &data.config.db_url;
    let mut client =
        postgres::Client::connect(db_url, postgres::NoTls).map_err(|e| (e.to_string(), true))?;
    f(&mut client).map_err(|e| (e.to_string(), false))
}

unsafe extern "C" fn cb_query_db(
    ctx_handle: u64,
    sql: *const std::ffi::c_char,
    params_json: *const std::ffi::c_char,
    out_json: *mut *mut std::ffi::c_char,
    out_err: *mut *mut std::ffi::c_char,
) -> bool {
    let sql_str = match unsafe { CStr::from_ptr(sql) }.to_str() {
        Ok(s) => s,
        Err(_) => return false,
    };

    let params = deserialize_db_params(params_json);
    let pg_params = postgres_params_from_dbvalue(&params);
    let param_refs: Vec<&(dyn postgres::types::ToSql + Sync)> =
        pg_params.iter().map(|p| p.as_ref()).collect();

    match with_db(ctx_handle, |client| client.query(sql_str, &param_refs)) {
        Ok(rows) => {
            let json_rows: Vec<serde_json::Value> = rows
                .iter()
                .map(|row| {
                    let mut map = serde_json::Map::new();
                    for (i, col) in row.columns().iter().enumerate() {
                        let name = col.name();
                        let value: serde_json::Value = row
                            .try_get::<_, serde_json::Value>(i)
                            .unwrap_or(serde_json::Value::Null);
                        map.insert(name.to_string(), value);
                    }
                    serde_json::Value::Object(map)
                })
                .collect();

            let json = serde_json::to_string(&json_rows).unwrap_or_else(|_| "[]".to_string());
            unsafe { *out_json = CString::new(json).unwrap().into_raw() }
            true
        }
        Err((e, _)) => {
            let err = CString::new(e).unwrap();
            unsafe { *out_err = err.into_raw() }
            false
        }
    }
}

unsafe extern "C" fn cb_execute_db(
    ctx_handle: u64,
    sql: *const std::ffi::c_char,
    params_json: *const std::ffi::c_char,
    out_rows: *mut u64,
    out_err: *mut *mut std::ffi::c_char,
) -> bool {
    let sql_str = match unsafe { CStr::from_ptr(sql) }.to_str() {
        Ok(s) => s,
        Err(_) => return false,
    };

    let params = deserialize_db_params(params_json);
    let pg_params = postgres_params_from_dbvalue(&params);
    let param_refs: Vec<&(dyn postgres::types::ToSql + Sync)> =
        pg_params.iter().map(|p| p.as_ref()).collect();

    match with_db(ctx_handle, |client| client.execute(sql_str, &param_refs)) {
        Ok(rows) => {
            unsafe { *out_rows = rows };
            true
        }
        Err((e, _)) => {
            let err = CString::new(e).unwrap();
            unsafe { *out_err = err.into_raw() }
            false
        }
    }
}

// ---- Channel message callback ----

unsafe extern "C" fn cb_send_channel_message(
    ctx_handle: u64,
    channel_id: u64,
    payload_json: *const std::ffi::c_char,
    out_message_id: *mut u64,
    out_err: *mut *mut std::ffi::c_char,
) -> bool {
    let ctx = match host_registry::get(ctx_handle) {
        Some(ctx) => ctx,
        None => return false,
    };

    let json_str = match unsafe { CStr::from_ptr(payload_json) }.to_str() {
        Ok(s) => s,
        Err(_) => return false,
    };

    let payload: pwr_bot_sdk::ResponsePayload = match serde_json::from_str(json_str) {
        Ok(p) => p,
        Err(e) => {
            let err = CString::new(format!("invalid payload JSON: {e}")).unwrap();
            if !out_err.is_null() {
                unsafe { *out_err = err.into_raw() };
            }
            return false;
        }
    };

    let http = ctx.http();
    let msg_fut = async {
        let mut builder = poise::serenity_prelude::CreateMessage::new();
        if let Some(content) = &payload.content {
            builder = builder.content(content);
        }
        http.send_message(
            poise::serenity_prelude::ChannelId::new(channel_id).into(),
            vec![],
            &builder,
        )
        .await
    };
    let handle = tokio::runtime::Handle::current();
    match handle.block_on(msg_fut) {
        Ok(msg) => {
            unsafe { *out_message_id = msg.id.get() };
            true
        }
        Err(e) => {
            let err = CString::new(e.to_string()).unwrap();
            if !out_err.is_null() {
                unsafe { *out_err = err.into_raw() };
            }
            false
        }
    }
}

// ---- DM callback ----

unsafe extern "C" fn cb_send_dm(
    ctx_handle: u64,
    user_id: u64,
    payload_json: *const std::ffi::c_char,
    out_message_id: *mut u64,
    out_err: *mut *mut std::ffi::c_char,
) -> bool {
    let ctx = match host_registry::get(ctx_handle) {
        Some(ctx) => ctx,
        None => return false,
    };

    let json_str = match unsafe { CStr::from_ptr(payload_json) }.to_str() {
        Ok(s) => s,
        Err(_) => return false,
    };

    let payload: pwr_bot_sdk::ResponsePayload = match serde_json::from_str(json_str) {
        Ok(p) => p,
        Err(e) => {
            let err = CString::new(format!("invalid DM payload JSON: {e}")).unwrap();
            if !out_err.is_null() {
                unsafe { *out_err = err.into_raw() };
            }
            return false;
        }
    };

    let http = ctx.http();
    let handle = tokio::runtime::Handle::current();
    let dm_fut = async {
        let user = http.get_user(UserId::new(user_id)).await?;
        let mut builder = CreateMessage::new();
        if let Some(content) = &payload.content {
            builder = builder.content(content);
        }
        user.id.dm(&http, builder).await
    };

    match handle.block_on(dm_fut) {
        Ok(msg) => {
            unsafe { *out_message_id = msg.id.get() };
            true
        }
        Err(e) => {
            let err = CString::new(e.to_string()).unwrap();
            if !out_err.is_null() {
                unsafe { *out_err = err.into_raw() };
            }
            false
        }
    }
}

// ---- Event callback ----

unsafe extern "C" fn cb_publish_event(
    ctx_handle: u64,
    event_name: *const std::ffi::c_char,
    payload_json: *const std::ffi::c_char,
    out_err: *mut *mut std::ffi::c_char,
) -> bool {
    let name = match unsafe { CStr::from_ptr(event_name) }.to_str() {
        Ok(n) => n,
        Err(_) => return false,
    };
    let payload_str = match unsafe { CStr::from_ptr(payload_json) }.to_str() {
        Ok(s) => s,
        Err(_) => return false,
    };
    let payload: serde_json::Value = match serde_json::from_str(payload_str) {
        Ok(v) => v,
        Err(e) => {
            let err = CString::new(format!("invalid event payload: {e}")).unwrap();
            if !out_err.is_null() {
                unsafe { *out_err = err.into_raw() };
            }
            return false;
        }
    };

    let data = match ctx_data(ctx_handle) {
        Some(d) => d,
        None => return false,
    };

    match data.event_bus.publish_named(name, payload) {
        Ok(()) => true,
        Err(e) => {
            let err = CString::new(e.to_string()).unwrap();
            if !out_err.is_null() {
                unsafe { *out_err = err.into_raw() };
            }
            false
        }
    }
}

// ---- Config callbacks ----

unsafe extern "C" fn cb_get_poll_interval(ctx_handle: u64) -> u64 {
    ctx_data(ctx_handle)
        .map(|data| data.config.poll_interval.as_secs())
        .unwrap_or(60)
}

unsafe extern "C" fn cb_get_data_path(
    ctx_handle: u64,
    out: *mut *mut std::ffi::c_char,
) -> bool {
    match ctx_data(ctx_handle) {
        Some(data) => {
            let path = data.config.data_path.to_string_lossy().to_string();
            let c_str = CString::new(path).unwrap();
            unsafe { *out = c_str.into_raw() };
            true
        }
        None => false,
    }
}

unsafe extern "C" fn cb_is_feature_enabled(
    ctx_handle: u64,
    feature: *const std::ffi::c_char,
) -> bool {
    let feature_name = match unsafe { CStr::from_ptr(feature) }.to_str() {
        Ok(n) => n,
        Err(_) => return false,
    };

    let data = match ctx_data(ctx_handle) {
        Some(d) => d,
        None => return false,
    };

    match feature_name {
        "voice_tracking" => data.config.features.voice_tracking,
        "feed_publisher" => data.config.features.feed_publisher,
        "autoregister_cmds" => data.config.features.autoregister_cmds,
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// HostCtx impl (used by built-in commands, not FFI)
// ---------------------------------------------------------------------------

#[async_trait]
impl HostCtx for FfiHostCtx {
    fn guild_id(&self) -> Option<u64> {
        self.inner.guild_id()
    }

    fn author_id(&self) -> u64 {
        self.inner.author_id()
    }

    fn channel_id(&self) -> u64 {
        self.inner.channel_id()
    }

    fn data(&self) -> Arc<Data> {
        self.inner.data()
    }

    async fn defer(&self) -> Result<(), Error> {
        self.inner.defer().await
    }

    async fn send_message(&self, payload: &MessagePayload) -> Result<MessageId, Error> {
        self.inner.send_message(payload).await
    }

    async fn edit_message(
        &self,
        message_id: MessageId,
        payload: &MessagePayload,
    ) -> Result<(), Error> {
        self.inner.edit_message(message_id, payload).await
    }

    async fn acknowledge(&self, interaction: &ComponentInteraction) -> Result<(), Error> {
        self.inner.acknowledge(interaction).await
    }
}
