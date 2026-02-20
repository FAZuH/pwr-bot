//! Chart generation for voice stats.

use std::collections::HashMap;
use std::collections::HashSet;

use chrono::DateTime;
use chrono::Datelike;
use chrono::Timelike;
use chrono::Utc;
use image::ImageEncoder;
use plotters::prelude::*;

use crate::bot::commands::voice::GuildStatType;
use crate::bot::commands::voice::VoiceStatsTimeRange;
use crate::model::VoiceSessionsModel;

/// Compute duration from join to leave
fn duration_secs(session: &VoiceSessionsModel, now: DateTime<Utc>) -> i64 {
    let leave = if session.leave_time == session.join_time {
        now
    } else {
        session.leave_time
    };
    (leave - session.join_time).num_seconds().clamp(0, 86400)
}

/// Generate a line chart for the given time range and aggregation
pub fn generate_line_chart(
    sessions: &[VoiceSessionsModel],
    time_range: VoiceStatsTimeRange,
    stat_type: GuildStatType,
    is_user: bool,
) -> anyhow::Result<Vec<u8>> {
    let now = Utc::now();

    // Groupings: map of (line_idx, x_val) -> stat
    // For User / TotalTime / AverageTime: stat is total seconds
    // For ActiveUserCount: stat is HashSet<user_id> (to count unique users)
    let mut time_map: HashMap<(u32, u32), i64> = HashMap::new();
    let mut user_map: HashMap<(u32, u32), HashSet<u64>> = HashMap::new();

    // Identify boundaries
    let mut x_min = 0;
    let mut x_max = 0;
    let mut x_labels = vec![];

    match time_range {
        VoiceStatsTimeRange::Hourly => {
            x_min = 0;
            x_max = 23;
            for i in 0..=23 {
                x_labels.push(format!("{:02}:00", i));
            }
        }
        VoiceStatsTimeRange::Weekly => {
            x_min = 0;
            x_max = 6;
            x_labels = vec!["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"]
                .into_iter()
                .map(|s| s.to_string())
                .collect();
        }
        VoiceStatsTimeRange::Monthly => {
            x_min = 1;
            x_max = 31;
            for i in 1..=31 {
                x_labels.push(format!("{}", i));
            }
        }
        _ => {}
    }

    for session in sessions {
        let (line_idx, x_val) = match time_range {
            VoiceStatsTimeRange::Hourly => {
                let days_ago =
                    (now.date_naive() - session.join_time.date_naive()).num_days() as u32;
                if days_ago > 3 {
                    continue;
                }
                (days_ago, session.join_time.hour())
            }
            VoiceStatsTimeRange::Weekly => {
                let diff_days = (now.date_naive() - session.join_time.date_naive()).num_days();
                // week offset
                let weeks_ago = (diff_days + now.weekday().num_days_from_monday() as i64) / 7;
                if !(0..=3).contains(&weeks_ago) {
                    continue;
                }
                (
                    weeks_ago as u32,
                    session.join_time.weekday().num_days_from_monday(),
                )
            }
            VoiceStatsTimeRange::Monthly => {
                let months_ago = (now.year() - session.join_time.year()) * 12
                    + (now.month() as i32 - session.join_time.month() as i32);
                if !(0..=3).contains(&months_ago) {
                    continue;
                }
                (months_ago as u32, session.join_time.day())
            }
            _ => (0, 0),
        };

        let secs = duration_secs(session, now);
        *time_map.entry((line_idx, x_val)).or_insert(0) += secs;
        user_map
            .entry((line_idx, x_val))
            .or_default()
            .insert(session.user_id);
    }

    // Convert to values (y_val)
    let mut max_y = 0.0f64;
    let mut series_data: Vec<Vec<(u32, f64)>> = vec![vec![]; 4];

    for line_idx in 0..=3 {
        for x_val in x_min..=x_max {
            let val;
            if stat_type == GuildStatType::ActiveUserCount && !is_user {
                val = user_map
                    .get(&(line_idx, x_val))
                    .map(|s| s.len() as f64)
                    .unwrap_or(0.0);
            } else if stat_type == GuildStatType::AverageTime && !is_user {
                let secs = *time_map.get(&(line_idx, x_val)).unwrap_or(&0) as f64;
                let users = user_map
                    .get(&(line_idx, x_val))
                    .map(|s| s.len() as f64)
                    .unwrap_or(1.0)
                    .max(1.0);
                val = (secs / 3600.0) / users; // hours
            } else {
                let secs = *time_map.get(&(line_idx, x_val)).unwrap_or(&0) as f64;
                val = secs / 3600.0; // hours
            }
            if val > max_y {
                max_y = val;
            }
            series_data[line_idx as usize].push((x_val, val));
        }
    }

    // Mean line (index 4)
    let mut mean_data = vec![];
    for x_val in x_min..=x_max {
        let mut sum = 0.0_f64;
        let mut count = 0.0_f64;
        for line_idx in 0..=3 {
            sum += series_data[line_idx as usize][(x_val - x_min) as usize].1;
            count += 1.0;
        }
        let mean = sum / f64::max(count, 1.0);
        if mean > max_y {
            max_y = mean;
        }
        mean_data.push((x_val, mean));
    }
    series_data.push(mean_data);

    // Pad max_y
    max_y = (max_y * 1.1).max(1.0);

    let mut buffer = vec![0; 800 * 400 * 3];
    {
        let root = BitMapBackend::with_buffer(&mut buffer, (800, 400)).into_drawing_area();
        root.fill(&RGBColor(43, 45, 49))?;

        let mut chart = ChartBuilder::on(&root)
            .margin(20)
            .x_label_area_size(40)
            .y_label_area_size(50)
            .build_cartesian_2d(x_min..x_max, 0.0..max_y)?;

        chart
            .configure_mesh()
            .disable_x_mesh()
            .disable_y_mesh()
            .x_desc(match time_range {
                VoiceStatsTimeRange::Hourly => "Hour of Day",
                VoiceStatsTimeRange::Weekly => "Day of Week",
                VoiceStatsTimeRange::Monthly => "Day of Month",
                _ => "",
            })
            .y_desc(if stat_type == GuildStatType::ActiveUserCount && !is_user {
                "Users"
            } else {
                "Hours"
            })
            .label_style(("sans-serif", 15).into_font().color(&WHITE))
            .x_label_formatter(&|x| {
                if *x >= x_min && *x <= x_max {
                    x_labels[(*x - x_min) as usize].clone()
                } else {
                    "".to_string()
                }
            })
            .axis_style(WHITE)
            .draw()?;

        let colors = [
            &RGBColor(128, 128, 128), // 3 periods ago (grey)
            &RGBColor(180, 180, 180), // 2 periods ago (light grey)
            &RGBColor(152, 195, 121), // 1 period ago (greenish)
            &RGBColor(97, 175, 239),  // current period (blue)
            &RGBColor(229, 192, 123), // mean line (yellow/orange)
        ];

        let labels = match time_range {
            VoiceStatsTimeRange::Hourly => vec![
                "Current Day",
                "1 Day Ago",
                "2 Days Ago",
                "3 Days Ago",
                "Mean",
            ],
            VoiceStatsTimeRange::Weekly => vec![
                "Current Week",
                "1 Week Ago",
                "2 Weeks Ago",
                "3 Weeks Ago",
                "Mean",
            ],
            VoiceStatsTimeRange::Monthly => vec![
                "Current Month",
                "1 Month Ago",
                "2 Months Ago",
                "3 Months Ago",
                "Mean",
            ],
            _ => vec!["", "", "", "", "Mean"],
        };

        for (i, series) in series_data.iter().enumerate() {
            // Lines are in reverse order of age: 0 = current, 1 = 1 ago, 2 = 2 ago, 3 = 3 ago
            // So we map line 3 -> colors[0], line 2 -> colors[1], line 1 -> colors[2], line 0 -> colors[3]
            // and line 4 (mean) -> colors[4]
            let color_idx = if i == 4 { 4 } else { 3 - i };

            let stroke_width = if i == 0 || i == 4 { 3 } else { 2 };
            let c = *colors[color_idx];

            chart
                .draw_series(LineSeries::new(
                    series.clone(),
                    c.stroke_width(stroke_width),
                ))?
                .label(labels[i])
                .legend(move |(x, y)| {
                    PathElement::new(vec![(x, y), (x + 20, y)], c.stroke_width(stroke_width))
                });
        }

        chart
            .configure_series_labels()
            .background_style(RGBColor(30, 31, 34))
            .border_style(BLACK)
            .label_font(("sans-serif", 15).into_font().color(&WHITE))
            .position(SeriesLabelPosition::UpperLeft)
            .draw()?;

        root.present()?;
    }

    let mut png_bytes = Vec::new();
    let mut cursor = std::io::Cursor::new(&mut png_bytes);
    image::codecs::png::PngEncoder::new(&mut cursor).write_image(
        &buffer,
        800,
        400,
        image::ExtendedColorType::Rgb8,
    )?;

    Ok(png_bytes)
}

#[cfg(test)]
mod tests {

    use chrono::Duration;
    use chrono::Utc;

    use super::*;

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
        assert_eq!(duration_secs(&session, now), 86400);

        // create a normal 2h session
        let session2 = VoiceSessionsModel {
            id: 2,
            user_id: 1,
            guild_id: 1,
            channel_id: 1,
            join_time: now - Duration::hours(3),
            leave_time: now - Duration::hours(1),
        };
        assert_eq!(duration_secs(&session2, now), 7200);
    }
}
