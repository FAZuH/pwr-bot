use deadpool_postgres::Pool;
use tokio_postgres::types::ToSql;

pub fn rows_to_json(rows: &[tokio_postgres::Row]) -> serde_json::Value {
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
    serde_json::Value::Array(json_rows)
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
