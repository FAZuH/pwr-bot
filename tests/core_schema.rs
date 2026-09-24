use diesel::QueryableByName;
use diesel_async::RunQueryDsl;
use pwr_bot::repo::PgRepos;

#[path = "support/db.rs"]
mod db;

#[derive(QueryableByName)]
struct TableExists {
    #[diesel(sql_type = diesel::sql_types::Bool)]
    exists: bool,
}

#[tokio::test]
async fn core_migrations_create_the_voice_sessions_table() {
    let db_url = db::db_url().await;
    let repos = PgRepos::new(db_url)
        .await
        .expect("connect to test database");
    repos.run_migrations().await.expect("run core migrations");

    let mut connection = repos.pool().get().await.expect("get database connection");
    let result: TableExists = diesel::sql_query(
        "SELECT EXISTS (SELECT FROM information_schema.tables \
         WHERE table_schema = 'public' AND table_name = 'voice_sessions')",
    )
    .get_result(&mut connection)
    .await
    .expect("query core schema");

    assert!(result.exists, "core migrations must create voice_sessions");
}
