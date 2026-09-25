pub fn day_key_label(ms: i64) -> (String, String) {
    let dt = glib::DateTime::from_unix_local_usec(ms.saturating_mul(1000))
        .unwrap_or_else(|_| glib::DateTime::now_local().expect("lokale Zeitzone"));
    let key = dt
        .format("%Y-%m-%d")
        .map(|s| s.to_string())
        .unwrap_or_default();
    let now = glib::DateTime::now_local().expect("lokale Zeitzone");
    let today = now
        .format("%Y-%m-%d")
        .map(|s| s.to_string())
        .unwrap_or_default();
    let yest = now
        .add_days(-1)
        .ok()
        .and_then(|d| d.format("%Y-%m-%d").ok())
        .map(|s| s.to_string())
        .unwrap_or_default();
    let label = if key == today {
        crate::tr!("Heute", "Today").to_string()
    } else if key == yest {
        crate::tr!("Gestern", "Yesterday").to_string()
    } else {
        format_date(&dt, crate::strings::current(), false)
    };
    (key, label)
}

pub fn fmt_time(ms: i64) -> String {
    let dt = glib::DateTime::from_unix_local_usec(ms.saturating_mul(1000)).ok();
    dt.and_then(|d| d.format("%H:%M").map(|s| s.to_string()).ok())
        .unwrap_or_default()
}

pub fn fmt_full(ms: i64) -> String {
    let dt = glib::DateTime::from_unix_local_usec(ms.saturating_mul(1000)).ok();
    dt.map(|d| {
        format!(
            "{}, {}",
            format_date(&d, crate::strings::current(), true),
            d.format("%H:%M").unwrap_or_default()
        )
    })
    .unwrap_or_default()
}

fn format_date(dt: &glib::DateTime, lang: crate::strings::Lang, with_year: bool) -> String {
    use crate::strings::Lang;
    let (weekdays, months) = match lang {
        Lang::En => (
            [
                "Monday",
                "Tuesday",
                "Wednesday",
                "Thursday",
                "Friday",
                "Saturday",
                "Sunday",
            ],
            [
                "January",
                "February",
                "March",
                "April",
                "May",
                "June",
                "July",
                "August",
                "September",
                "October",
                "November",
                "December",
            ],
        ),
        Lang::De => (
            [
                "Montag",
                "Dienstag",
                "Mittwoch",
                "Donnerstag",
                "Freitag",
                "Samstag",
                "Sonntag",
            ],
            [
                "Januar",
                "Februar",
                "März",
                "April",
                "Mai",
                "Juni",
                "Juli",
                "August",
                "September",
                "Oktober",
                "November",
                "Dezember",
            ],
        ),
    };
    let weekday = weekdays[(dt.day_of_week() - 1) as usize];
    let month = months[(dt.month() - 1) as usize];
    let day = dt.day_of_month();
    let date = match lang {
        Lang::En => format!("{weekday}, {month} {day}"),
        Lang::De => format!("{weekday}, {day}. {month}"),
    };
    if with_year {
        format!("{date} {}", dt.year())
    } else {
        date
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dates_follow_app_language_instead_of_process_locale() {
        let dt = glib::DateTime::from_iso8601("2026-09-25T12:00:00Z", None).unwrap();
        assert_eq!(
            format_date(&dt, crate::strings::Lang::En, true),
            "Friday, September 25 2026"
        );
        assert_eq!(
            format_date(&dt, crate::strings::Lang::De, false),
            "Freitag, 25. September"
        );
    }
}
