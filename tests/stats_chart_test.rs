#[cfg(test)]
mod tests {
    use pwr_bot::bot::commands::voice::stats_chart::*;
    use pwr_bot::bot::commands::voice::{GuildStatType, VoiceStatsTimeRange};
    use pwr_bot::model::VoiceSessionsModel;
    use chrono::{TimeZone, Utc, Duration, Datelike};

    #[test]
    fn test_duration_capping() {
        let now = Utc::now();
        // create a ghost session 30 days ago
        let session = VoiceSessionsModel {
            id: 1,
            user_id: 1,
            guild_id: 1,
            channel_id: 1,
            join_time: now - Duration::days(30),
            leave_time: now - Duration::days(30), // active ghost
        };
        // It shouldn't sum 30 days of seconds, it should cap at 86400 (24h)
        assert_eq!(super::duration_secs(&session, now), 86400);

        // create a normal 2h session
        let session2 = VoiceSessionsModel {
            id: 2,
            user_id: 1,
            guild_id: 1,
            channel_id: 1,
            join_time: now - Duration::hours(3),
            leave_time: now - Duration::hours(1),
        };
        assert_eq!(super::duration_secs(&session2, now), 7200);
    }
}
