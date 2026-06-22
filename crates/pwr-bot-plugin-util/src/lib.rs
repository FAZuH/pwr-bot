use deadpool_postgres::Pool;
use tokio_postgres::types::ToSql;

/// Converts PostgreSQL rows into a JSON array of objects (one per row).
///
/// Handles common scalar types (`int2`, `int4`, `int8`, `bool`, `text`,
/// `varchar`, `float4`, `float8`) with typed `FromSql` reads so that integer
/// columns produce JSON numbers instead of `Null` (which happens when using
/// `serde_json::Value`'s `FromSql` impl, which only supports JSON/JSONB).
///
/// Nullable columns are handled gracefully — `NULL` becomes `Value::Null`.
/// Unknown types fall back to `serde_json::Value`'s `FromSql` impl (JSON/JSONB)
/// and then to `Null` if that also fails.
pub fn rows_to_json(rows: &[tokio_postgres::Row]) -> serde_json::Value {
    serde_json::Value::Array(rows_to_json_vec(rows))
}

/// Same as [`rows_to_json`] but returns the inner `Vec` directly.
pub fn rows_to_json_vec(rows: &[tokio_postgres::Row]) -> Vec<serde_json::Value> {
    rows.iter()
        .map(|row| {
            let mut map = serde_json::Map::new();
            for (i, col) in row.columns().iter().enumerate() {
                map.insert(col.name().to_string(), column_to_json(row, i, col.type_()));
            }
            serde_json::Value::Object(map)
        })
        .collect()
}

/// Converts a single column to a JSON value based on its PostgreSQL type.
fn column_to_json(
    row: &tokio_postgres::Row,
    idx: usize,
    ty: &tokio_postgres::types::Type,
) -> serde_json::Value {
    match ty.name() {
        "int2" => row
            .try_get::<_, Option<i16>>(idx)
            .ok()
            .flatten()
            .map(|v| serde_json::Value::from(v as i64))
            .unwrap_or(serde_json::Value::Null),
        "int4" => row
            .try_get::<_, Option<i32>>(idx)
            .ok()
            .flatten()
            .map(|v| serde_json::Value::from(v as i64))
            .unwrap_or(serde_json::Value::Null),
        "int8" => row
            .try_get::<_, Option<i64>>(idx)
            .ok()
            .flatten()
            .map(serde_json::Value::from)
            .unwrap_or(serde_json::Value::Null),
        "bool" => row
            .try_get::<_, Option<bool>>(idx)
            .ok()
            .flatten()
            .map(serde_json::Value::from)
            .unwrap_or(serde_json::Value::Null),
        "text" | "varchar" => row
            .try_get::<_, Option<String>>(idx)
            .ok()
            .flatten()
            .map(serde_json::Value::from)
            .unwrap_or(serde_json::Value::Null),
        "float4" => row
            .try_get::<_, Option<f32>>(idx)
            .ok()
            .flatten()
            .map(|v| serde_json::Value::from(v as f64))
            .unwrap_or(serde_json::Value::Null),
        "float8" => row
            .try_get::<_, Option<f64>>(idx)
            .ok()
            .flatten()
            .map(serde_json::Value::from)
            .unwrap_or(serde_json::Value::Null),
        _ => row
            .try_get::<_, serde_json::Value>(idx)
            .unwrap_or(serde_json::Value::Null),
    }
}

pub async fn query_json(
    pool: &Pool,
    sql: &str,
    params: &[&(dyn ToSql + Sync)],
) -> Result<serde_json::Value, String> {
    let client = pool.get().await.map_err(|e| e.to_string())?;
    let rows = client.query(sql, params).await.map_err(|e| e.to_string())?;
    Ok(rows_to_json(&rows))
}

pub async fn db_execute(
    pool: &Pool,
    sql: &str,
    params: &[&(dyn ToSql + Sync)],
) -> Result<u64, String> {
    let client = pool.get().await.map_err(|e| e.to_string())?;
    client.execute(sql, params).await.map_err(|e| e.to_string())
}
