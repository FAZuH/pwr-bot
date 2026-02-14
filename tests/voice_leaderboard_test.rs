//! Integration tests for voice leaderboard commands.

use chrono::Datelike;
use chrono::Utc;
use pwr_bot::bot::commands::voice::VoiceLeaderboardTimeRange;
use pwr_bot::bot::utils::format_duration;

#[test]
fn test_format_duration_edge_cases() {
    // Test zero
    assert_eq!(format_duration(0), "0s");

    // Test boundaries
    assert_eq!(format_duration(59), "59s");
    assert_eq!(format_duration(60), "1m");
    assert_eq!(format_duration(61), "1m");

    assert_eq!(format_duration(3599), "59m");
    assert_eq!(format_duration(3600), "1h");
    assert_eq!(format_duration(3601), "1h");

    assert_eq!(format_duration(86399), "23h 59m");
    assert_eq!(format_duration(86400), "1d");
    assert_eq!(format_duration(86401), "1d");
}

#[test]
fn test_format_duration_comprehensive() {
    // Seconds
    assert_eq!(format_duration(45), "45s");

    // Minutes
    assert_eq!(format_duration(300), "5m");
    assert_eq!(format_duration(1500), "25m");

    // Hours with and without minutes
    assert_eq!(format_duration(7200), "2h");
    assert_eq!(format_duration(7260), "2h 1m");
    assert_eq!(format_duration(9000), "2h 30m");

    // Days with and without hours
    assert_eq!(format_duration(172800), "2d");
    assert_eq!(format_duration(176400), "2d 1h");
    assert_eq!(format_duration(259200), "3d");

    // Large values
    assert_eq!(format_duration(604800), "7d"); // One week
    assert_eq!(format_duration(2592000), "30d"); // ~30 days
    assert_eq!(format_duration(31536000), "365d"); // ~1 year
}

#[test]
fn test_voice_leaderboard_time_range_all_variants() {
    // Test all time range variants can be converted to datetime
    let ranges = vec![
        VoiceLeaderboardTimeRange::Today,
        VoiceLeaderboardTimeRange::Past3Days,
        VoiceLeaderboardTimeRange::ThisWeek,
        VoiceLeaderboardTimeRange::Past2Weeks,
        VoiceLeaderboardTimeRange::ThisMonth,
        VoiceLeaderboardTimeRange::ThisYear,
        VoiceLeaderboardTimeRange::AllTime,
    ];

    let now = Utc::now();

    for range in ranges {
        let (since, until) = range.to_range();
        assert!(since <= until, "Time range {:?} has since > until", range);
        assert!(
            until >= now || range == VoiceLeaderboardTimeRange::AllTime,
            "Until should be around now for {:?}",
            range
        );

        // Verify round-trip through name
        let name = range.name();
        let recovered = VoiceLeaderboardTimeRange::from_name(name);
        assert!(
            recovered.is_some(),
            "Should be able to recover {:?} from name '{}'",
            range,
            name
        );
        assert_eq!(
            recovered.unwrap() as i32,
            range as i32,
            "Recovered range should match original"
        );
    }
}

#[test]
fn test_time_range_date_boundaries() {
    let now = Utc::now();

    // Today should start at midnight today
    let today_start: chrono::DateTime<chrono::Utc> = VoiceLeaderboardTimeRange::Today.into();
    assert_eq!(
        today_start.time(),
        chrono::NaiveTime::from_hms_opt(0, 0, 0).unwrap()
    );
    assert_eq!(today_start.date_naive(), now.date_naive());

    // This month should start on the 1st
    let month_start: chrono::DateTime<chrono::Utc> = VoiceLeaderboardTimeRange::ThisMonth.into();
    assert_eq!(month_start.day(), 1);
    assert_eq!(
        month_start.time(),
        chrono::NaiveTime::from_hms_opt(0, 0, 0).unwrap()
    );

    // This year should start on Jan 1
    let year_start: chrono::DateTime<chrono::Utc> = VoiceLeaderboardTimeRange::ThisYear.into();
    assert_eq!(year_start.month(), 1);
    assert_eq!(year_start.day(), 1);

    // All time should be Unix epoch
    let all_time_start: chrono::DateTime<chrono::Utc> = VoiceLeaderboardTimeRange::AllTime.into();
    assert_eq!(all_time_start, chrono::DateTime::UNIX_EPOCH);
}

#[test]
fn test_time_range_relative_durations() {
    let now = Utc::now();

    // Past 3 days should be approximately 3 days ago
    let (since, _) = VoiceLeaderboardTimeRange::Past3Days.to_range();
    let duration = now.signed_duration_since(since);
    // Should be between 2 and 4 days (accounting for time-of-day variations)
    assert!(
        duration.num_days() >= 2 && duration.num_days() <= 4,
        "Past 3 days should be roughly 3 days ago, got {} days",
        duration.num_days()
    );

    // This week should be within the last 7 days
    let (since, _) = VoiceLeaderboardTimeRange::ThisWeek.to_range();
    let duration = now.signed_duration_since(since);
    assert!(
        duration.num_days() >= 0 && duration.num_days() <= 7,
        "This week should be within last 7 days, got {} days",
        duration.num_days()
    );

    // Past 2 weeks should be within the last 14 days
    let (since, _) = VoiceLeaderboardTimeRange::Past2Weeks.to_range();
    let duration = now.signed_duration_since(since);
    assert!(
        duration.num_days() >= 7 && duration.num_days() <= 14,
        "Past 2 weeks should be within last 14 days, got {} days",
        duration.num_days()
    );
}

#[test]
fn test_leaderboard_entry_clone() {
    use pwr_bot::bot::commands::voice::image_builder::LeaderboardEntry;

    let entry = LeaderboardEntry {
        rank: 1,
        user_id: 123456789012345678,
        display_name: "Test User".to_string(),
        avatar_url: "https://cdn.discordapp.com/avatars/123/abc.png".to_string(),
        duration_seconds: 3600,
        avatar_image: None,
    };

    let cloned = entry.clone();
    assert_eq!(cloned.rank, entry.rank);
    assert_eq!(cloned.user_id, entry.user_id);
    assert_eq!(cloned.display_name, entry.display_name);
    assert_eq!(cloned.avatar_url, entry.avatar_url);
    assert_eq!(cloned.duration_seconds, entry.duration_seconds);
    assert!(cloned.avatar_image.is_none());
}
