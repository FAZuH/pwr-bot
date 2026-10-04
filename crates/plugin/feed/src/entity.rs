use chrono::DateTime;
use chrono::Utc;
use diesel::backend::Backend;
use diesel::deserialize::FromSql;
use diesel::deserialize::FromSqlRow;
use diesel::expression::AsExpression;
use diesel::prelude::*;
use diesel::serialize::ToSql;
use diesel::sql_types::Integer;
use diesel::sql_types::Nullable;
use diesel::sql_types::Text;
use diesel::sql_types::Timestamptz;
use serde::Deserialize;
use serde::Serialize;

use crate::repo::schema::feed_items;
use crate::repo::schema::feed_subscriptions;
use crate::repo::schema::feeds;
use crate::repo::schema::subscribers;

#[derive(
    Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq, AsExpression, FromSqlRow,
)]
#[diesel(sql_type = Text)]
#[serde(rename_all = "lowercase")]
pub enum SubscriberType {
    #[default]
    Guild,
    Dm,
}

impl<B> ToSql<Text, B> for SubscriberType
where
    B: Backend,
    str: ToSql<Text, B>,
{
    fn to_sql<'b>(
        &'b self,
        out: &mut diesel::serialize::Output<'b, '_, B>,
    ) -> diesel::serialize::Result {
        match self {
            SubscriberType::Guild => <str as ToSql<Text, B>>::to_sql("guild", out),
            SubscriberType::Dm => <str as ToSql<Text, B>>::to_sql("dm", out),
        }
    }
}

impl<B> FromSql<Text, B> for SubscriberType
where
    B: Backend,
    String: FromSql<Text, B>,
{
    fn from_sql(bytes: B::RawValue<'_>) -> diesel::deserialize::Result<Self> {
        match <String as FromSql<Text, B>>::from_sql(bytes)?.as_str() {
            "guild" => Ok(SubscriberType::Guild),
            "dm" => Ok(SubscriberType::Dm),
            other => Err(format!("unknown subscriber type: {other}").into()),
        }
    }
}

#[derive(Queryable, Selectable, Insertable, Identifiable, AsChangeset)]
#[diesel(table_name = feeds)]
#[diesel(check_for_backend(diesel::pg::Pg))]
#[derive(Serialize, Deserialize, Default, Clone, Debug)]
pub struct FeedEntity {
    pub id: i32,
    pub name: String,
    pub description: String,
    pub platform_id: String,
    pub source_id: String,
    pub items_id: String,
    pub source_url: String,
    pub cover_url: String,
    pub tags: String,
}

#[derive(Queryable, Selectable, Insertable, Identifiable, AsChangeset)]
#[diesel(table_name = feed_items)]
#[diesel(check_for_backend(diesel::pg::Pg))]
#[derive(Serialize, Deserialize, Default, Clone, Debug)]
pub struct FeedItemEntity {
    pub id: i32,
    pub feed_id: i32,
    pub description: String,
    pub published: DateTime<Utc>,
}

#[derive(Queryable, Selectable, Insertable, Identifiable, AsChangeset)]
#[diesel(table_name = subscribers)]
#[diesel(check_for_backend(diesel::pg::Pg))]
#[derive(Serialize, Deserialize, Default, Clone, Debug)]
pub struct SubscriberEntity {
    pub id: i32,
    #[diesel(column_name = type_)]
    pub r#type: SubscriberType,
    pub target_id: String,
}

#[derive(Queryable, Selectable, Insertable, Identifiable, AsChangeset)]
#[diesel(table_name = feed_subscriptions)]
#[diesel(check_for_backend(diesel::pg::Pg))]
#[derive(Serialize, Deserialize, Default, Clone, Debug)]
pub struct FeedSubscriptionEntity {
    pub id: i32,
    pub feed_id: i32,
    pub subscriber_id: i32,
}

#[derive(QueryableByName)]
pub struct FeedWithLatestItemRow {
    #[diesel(sql_type = Integer)]
    pub id: i32,
    #[diesel(sql_type = Text)]
    pub name: String,
    #[diesel(sql_type = Text)]
    pub description: String,
    #[diesel(sql_type = Text)]
    pub platform_id: String,
    #[diesel(sql_type = Text)]
    pub source_id: String,
    #[diesel(sql_type = Text)]
    pub items_id: String,
    #[diesel(sql_type = Text)]
    pub source_url: String,
    #[diesel(sql_type = Text)]
    pub cover_url: String,
    #[diesel(sql_type = Text)]
    pub tags: String,
    #[diesel(sql_type = Nullable<Integer>)]
    pub item_id: Option<i32>,
    #[diesel(sql_type = Nullable<Text>)]
    pub item_description: Option<String>,
    #[diesel(sql_type = Nullable<Timestamptz>)]
    pub item_published: Option<DateTime<Utc>>,
}
