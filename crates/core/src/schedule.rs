//! Scheduled snapshot task: command line, Task Scheduler XML and `schtasks` arguments.

/// Task name in Task Scheduler.
pub const TASK_NAME: &str = r"diskmap\Snapshot";
/// Day names as stored in the configuration, Monday first.
pub const DAYS: [&str; 7] = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"];
const XML_DAYS: [&str; 7] = [
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
    "Sunday",
];

/// What the task runs and when.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    /// Path of `diskmap.exe`.
    pub exe: String,
    pub roots: Vec<String>,
    /// Index in [`DAYS`] for a weekly task, `None` for daily.
    pub weekday: Option<usize>,
    pub hour: u8,
    pub minute: u8,
    /// Run with highest privileges (MFT backend).
    pub elevated: bool,
}

/// Hour and minute of `HH:MM`.
pub fn parse_time(s: &str) -> Option<(u8, u8)> {
    let (h, m) = s.trim().split_once(':')?;
    let (h, m) = (h.parse::<u8>().ok()?, m.parse::<u8>().ok()?);
    (h < 24 && m < 60 && s.trim().len() == 5).then_some((h, m))
}

/// Index of a day name of [`DAYS`].
pub fn day_index(s: &str) -> Option<usize> {
    DAYS.iter().position(|d| d.eq_ignore_ascii_case(s.trim()))
}

/// Quotes one argument for a Windows command line.
pub fn quote_arg(arg: &str) -> String {
    let mut out = String::from('"');
    let mut slashes = 0;
    for c in arg.chars() {
        match c {
            '\\' => slashes += 1,
            '"' => {
                out.push_str(&"\\".repeat(slashes * 2 + 1));
                out.push('"');
                slashes = 0;
            }
            _ => {
                out.push_str(&"\\".repeat(slashes));
                out.push(c);
                slashes = 0;
            }
        }
    }
    out.push_str(&"\\".repeat(slashes * 2));
    out.push('"');
    out
}

/// Quoted arguments joined by spaces.
pub fn command_line<S: AsRef<str>>(args: &[S]) -> String {
    args.iter()
        .map(|a| quote_arg(a.as_ref()))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Arguments of the headless run: `--snapshot <root>` for each root, then `--quiet`.
pub fn snapshot_args<S: AsRef<str>>(roots: &[S]) -> Vec<String> {
    let mut out = Vec::new();
    for r in roots {
        out.push("--snapshot".to_string());
        out.push(r.as_ref().to_string());
    }
    out.push("--quiet".into());
    out
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn schedule_xml(weekday: Option<usize>) -> String {
    match weekday {
        None => "<ScheduleByDay><DaysInterval>1</DaysInterval></ScheduleByDay>".into(),
        Some(d) => format!(
            "<ScheduleByWeek><DaysOfWeek><{} /></DaysOfWeek><WeeksInterval>1</WeeksInterval></ScheduleByWeek>",
            XML_DAYS[d.min(6)]
        ),
    }
}

/// Task definition for `schtasks /Create /XML`: runs on battery, catches up a missed start, one instance.
pub fn task_xml(plan: &Plan) -> String {
    let level = if plan.elevated {
        "HighestAvailable"
    } else {
        "LeastPrivilege"
    };
    format!(
        r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo><Description>diskmap snapshot</Description></RegistrationInfo>
  <Triggers>
    <CalendarTrigger>
      <StartBoundary>2024-01-01T{h:02}:{m:02}:00</StartBoundary>
      <Enabled>true</Enabled>
      {schedule}
    </CalendarTrigger>
  </Triggers>
  <Principals>
    <Principal id="Author"><LogonType>InteractiveToken</LogonType><RunLevel>{level}</RunLevel></Principal>
  </Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <StartWhenAvailable>true</StartWhenAvailable>
    <ExecutionTimeLimit>PT2H</ExecutionTimeLimit>
    <Priority>7</Priority>
  </Settings>
  <Actions Context="Author">
    <Exec><Command>{exe}</Command><Arguments>{args}</Arguments></Exec>
  </Actions>
</Task>
"#,
        h = plan.hour,
        m = plan.minute,
        schedule = schedule_xml(plan.weekday),
        exe = xml_escape(&plan.exe),
        args = xml_escape(&command_line(&snapshot_args(&plan.roots))),
    )
}

/// `schtasks` arguments that create or replace the task from an XML file.
pub fn create_args(xml_path: &str) -> Vec<String> {
    ["/Create", "/TN", TASK_NAME, "/XML", xml_path, "/F"]
        .map(String::from)
        .to_vec()
}

/// `schtasks` arguments that delete the task.
pub fn delete_args() -> Vec<String> {
    ["/Delete", "/TN", TASK_NAME, "/F"]
        .map(String::from)
        .to_vec()
}

/// `schtasks` arguments that succeed only when the task exists.
pub fn query_args() -> Vec<String> {
    ["/Query", "/TN", TASK_NAME].map(String::from).to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan() -> Plan {
        Plan {
            exe: r"C:\Apps\R&D\diskmap.exe".into(),
            roots: vec![r"C:\".into(), r"D:\my data".into()],
            weekday: None,
            hour: 3,
            minute: 5,
            elevated: false,
        }
    }

    #[test]
    fn times_and_days() {
        assert_eq!(parse_time("03:05"), Some((3, 5)));
        assert_eq!(parse_time("23:59"), Some((23, 59)));
        assert_eq!(parse_time("24:00"), None);
        assert_eq!(parse_time("3:05"), None);
        assert_eq!(parse_time("x"), None);
        assert_eq!(day_index("Sun"), Some(6));
        assert_eq!(day_index("noday"), None);
    }

    #[test]
    fn quotes_arguments() {
        assert_eq!(quote_arg(r"C:\"), r#""C:\\""#);
        assert_eq!(quote_arg(r"D:\my files\x"), r#""D:\my files\x""#);
        assert_eq!(quote_arg(r#"a"b"#), r#""a\"b""#);
        assert_eq!(command_line(&[r"C:\a", r"E:\"]), r#""C:\a" "E:\\""#);
    }

    #[test]
    fn snapshot_command_line() {
        let args = snapshot_args(&[r"C:\", r"D:\my data"]);
        assert_eq!(
            command_line(&args),
            r#""--snapshot" "C:\\" "--snapshot" "D:\my data" "--quiet""#
        );
    }

    #[test]
    fn daily_xml() {
        let xml = task_xml(&plan());
        assert!(xml.contains("<StartBoundary>2024-01-01T03:05:00</StartBoundary>"));
        assert!(xml.contains("<DaysInterval>1</DaysInterval>"));
        assert!(xml.contains("<RunLevel>LeastPrivilege</RunLevel>"));
        assert!(xml.contains(r"<Command>C:\Apps\R&amp;D\diskmap.exe</Command>"));
        assert!(xml.contains(r#"&quot;--snapshot&quot; &quot;C:\\&quot;"#));
        assert!(xml.contains("<DisallowStartIfOnBatteries>false"));
    }

    #[test]
    fn weekly_elevated_xml() {
        let p = Plan {
            weekday: Some(2),
            elevated: true,
            ..plan()
        };
        let xml = task_xml(&p);
        assert!(xml.contains("<DaysOfWeek><Wednesday /></DaysOfWeek>"));
        assert!(xml.contains("<RunLevel>HighestAvailable</RunLevel>"));
    }

    #[test]
    fn schtasks_args() {
        assert_eq!(
            create_args(r"C:\t\task.xml"),
            [
                "/Create",
                "/TN",
                r"diskmap\Snapshot",
                "/XML",
                r"C:\t\task.xml",
                "/F"
            ]
        );
        assert_eq!(delete_args(), ["/Delete", "/TN", r"diskmap\Snapshot", "/F"]);
        assert_eq!(query_args(), ["/Query", "/TN", r"diskmap\Snapshot"]);
    }
}
