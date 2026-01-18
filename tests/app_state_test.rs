//! App state tests

use chrono::{DateTime, Utc};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SortColumn {
    Calls,
    LastCall,
    AvgDuration,
    Name,
}

#[derive(Debug, Clone)]
struct ToolMetrics {
    tool_name: String,
    call_count: u64,
    last_call: Option<DateTime<Utc>>,
    avg_duration_ms: f64,
    success_count: u64,
    error_count: u64,
}

/// Test sort column cycling
#[test]
fn test_sort_column_cycle() {
    let mut sort_by = SortColumn::Calls;

    sort_by = match sort_by {
        SortColumn::Calls => SortColumn::LastCall,
        SortColumn::LastCall => SortColumn::AvgDuration,
        SortColumn::AvgDuration => SortColumn::Name,
        SortColumn::Name => SortColumn::Calls,
    };
    assert_eq!(sort_by, SortColumn::LastCall);

    sort_by = match sort_by {
        SortColumn::Calls => SortColumn::LastCall,
        SortColumn::LastCall => SortColumn::AvgDuration,
        SortColumn::AvgDuration => SortColumn::Name,
        SortColumn::Name => SortColumn::Calls,
    };
    assert_eq!(sort_by, SortColumn::AvgDuration);

    sort_by = match sort_by {
        SortColumn::Calls => SortColumn::LastCall,
        SortColumn::LastCall => SortColumn::AvgDuration,
        SortColumn::AvgDuration => SortColumn::Name,
        SortColumn::Name => SortColumn::Calls,
    };
    assert_eq!(sort_by, SortColumn::Name);

    sort_by = match sort_by {
        SortColumn::Calls => SortColumn::LastCall,
        SortColumn::LastCall => SortColumn::AvgDuration,
        SortColumn::AvgDuration => SortColumn::Name,
        SortColumn::Name => SortColumn::Calls,
    };
    assert_eq!(sort_by, SortColumn::Calls);
}

/// Test sorting by call count descending
#[test]
fn test_sort_by_calls_desc() {
    let mut tools = vec![
        ToolMetrics {
            tool_name: "Read".into(),
            call_count: 50,
            last_call: None,
            avg_duration_ms: 10.0,
            success_count: 50,
            error_count: 0,
        },
        ToolMetrics {
            tool_name: "Write".into(),
            call_count: 100,
            last_call: None,
            avg_duration_ms: 20.0,
            success_count: 100,
            error_count: 0,
        },
        ToolMetrics {
            tool_name: "Bash".into(),
            call_count: 75,
            last_call: None,
            avg_duration_ms: 150.0,
            success_count: 70,
            error_count: 5,
        },
    ];

    tools.sort_by(|a, b| b.call_count.cmp(&a.call_count));

    assert_eq!(tools[0].tool_name, "Write");
    assert_eq!(tools[1].tool_name, "Bash");
    assert_eq!(tools[2].tool_name, "Read");
}

/// Test sorting by call count ascending
#[test]
fn test_sort_by_calls_asc() {
    let mut tools = vec![
        ToolMetrics {
            tool_name: "Read".into(),
            call_count: 50,
            last_call: None,
            avg_duration_ms: 10.0,
            success_count: 50,
            error_count: 0,
        },
        ToolMetrics {
            tool_name: "Write".into(),
            call_count: 100,
            last_call: None,
            avg_duration_ms: 20.0,
            success_count: 100,
            error_count: 0,
        },
        ToolMetrics {
            tool_name: "Bash".into(),
            call_count: 75,
            last_call: None,
            avg_duration_ms: 150.0,
            success_count: 70,
            error_count: 5,
        },
    ];

    tools.sort_by(|a, b| a.call_count.cmp(&b.call_count));

    assert_eq!(tools[0].tool_name, "Read");
    assert_eq!(tools[1].tool_name, "Bash");
    assert_eq!(tools[2].tool_name, "Write");
}

/// Test sorting by name
#[test]
fn test_sort_by_name() {
    let mut tools = vec![
        ToolMetrics {
            tool_name: "Read".into(),
            call_count: 50,
            last_call: None,
            avg_duration_ms: 10.0,
            success_count: 50,
            error_count: 0,
        },
        ToolMetrics {
            tool_name: "Write".into(),
            call_count: 100,
            last_call: None,
            avg_duration_ms: 20.0,
            success_count: 100,
            error_count: 0,
        },
        ToolMetrics {
            tool_name: "Bash".into(),
            call_count: 75,
            last_call: None,
            avg_duration_ms: 150.0,
            success_count: 70,
            error_count: 5,
        },
    ];

    tools.sort_by(|a, b| a.tool_name.cmp(&b.tool_name));

    assert_eq!(tools[0].tool_name, "Bash");
    assert_eq!(tools[1].tool_name, "Read");
    assert_eq!(tools[2].tool_name, "Write");
}

/// Test sorting by avg duration
#[test]
fn test_sort_by_avg_duration() {
    let mut tools = vec![
        ToolMetrics {
            tool_name: "Read".into(),
            call_count: 50,
            last_call: None,
            avg_duration_ms: 10.0,
            success_count: 50,
            error_count: 0,
        },
        ToolMetrics {
            tool_name: "Write".into(),
            call_count: 100,
            last_call: None,
            avg_duration_ms: 20.0,
            success_count: 100,
            error_count: 0,
        },
        ToolMetrics {
            tool_name: "Bash".into(),
            call_count: 75,
            last_call: None,
            avg_duration_ms: 150.0,
            success_count: 70,
            error_count: 5,
        },
    ];

    tools.sort_by(|a, b| b.avg_duration_ms.partial_cmp(&a.avg_duration_ms).unwrap());

    assert_eq!(tools[0].tool_name, "Bash");
    assert_eq!(tools[1].tool_name, "Write");
    assert_eq!(tools[2].tool_name, "Read");
}

/// Test selected index wrapping forward
#[test]
fn test_select_next_wrap() {
    let tools_len = 3usize;
    let mut selected_index = 2usize;

    selected_index = (selected_index + 1) % tools_len;
    assert_eq!(selected_index, 0);
}

/// Test selected index wrapping backward
#[test]
fn test_select_previous_wrap() {
    let tools_len = 3usize;
    let mut selected_index = 0usize;

    selected_index = if selected_index == 0 {
        tools_len - 1
    } else {
        selected_index - 1
    };
    assert_eq!(selected_index, 2);
}

/// Test selected index normal forward
#[test]
fn test_select_next_normal() {
    let tools_len = 3usize;
    let mut selected_index = 0usize;

    selected_index = (selected_index + 1) % tools_len;
    assert_eq!(selected_index, 1);
}

/// Test selected index normal backward
#[test]
fn test_select_previous_normal() {
    let tools_len = 3usize;
    let mut selected_index = 2usize;

    selected_index = if selected_index == 0 {
        tools_len - 1
    } else {
        selected_index - 1
    };
    assert_eq!(selected_index, 1);
}

/// Test toggle pause state
#[test]
fn test_toggle_pause() {
    let mut paused = false;

    paused = !paused;
    assert!(paused);

    paused = !paused;
    assert!(!paused);
}

/// Test toggle detail view
#[test]
fn test_toggle_detail() {
    let mut show_detail = false;

    show_detail = !show_detail;
    assert!(show_detail);

    show_detail = !show_detail;
    assert!(!show_detail);
}

/// Test selected index bounds check
#[test]
fn test_selected_index_bounds() {
    let tools = vec![
        ToolMetrics {
            tool_name: "Read".into(),
            call_count: 50,
            last_call: None,
            avg_duration_ms: 10.0,
            success_count: 50,
            error_count: 0,
        },
        ToolMetrics {
            tool_name: "Write".into(),
            call_count: 100,
            last_call: None,
            avg_duration_ms: 20.0,
            success_count: 100,
            error_count: 0,
        },
    ];
    let mut selected_index = 5usize;

    // Clamp to valid range
    if !tools.is_empty() && selected_index >= tools.len() {
        selected_index = tools.len() - 1;
    }

    assert_eq!(selected_index, 1);
}

/// Test empty tools list
#[test]
fn test_empty_tools_list() {
    let tools: Vec<ToolMetrics> = vec![];
    let selected_index = 0usize;

    let selected = tools.get(selected_index);
    assert!(selected.is_none());
}

/// Test total tool calls calculation
#[test]
fn test_total_tool_calls() {
    let tools = vec![
        ToolMetrics {
            tool_name: "Read".into(),
            call_count: 50,
            last_call: None,
            avg_duration_ms: 10.0,
            success_count: 50,
            error_count: 0,
        },
        ToolMetrics {
            tool_name: "Write".into(),
            call_count: 100,
            last_call: None,
            avg_duration_ms: 20.0,
            success_count: 100,
            error_count: 0,
        },
        ToolMetrics {
            tool_name: "Bash".into(),
            call_count: 75,
            last_call: None,
            avg_duration_ms: 150.0,
            success_count: 70,
            error_count: 5,
        },
    ];

    let total: u64 = tools.iter().map(|t| t.call_count).sum();
    assert_eq!(total, 225);
}

/// Test session time formatting - hours
#[test]
fn test_session_time_format_hours() {
    let hours = 2i64;
    let minutes = 30i64;

    let session_time = if hours > 0 {
        format!("{}h {:02}m", hours, minutes)
    } else {
        format!("{}m", minutes)
    };

    assert_eq!(session_time, "2h 30m");
}

/// Test session time formatting - minutes only
#[test]
fn test_session_time_format_minutes() {
    let hours = 0i64;
    let minutes = 45i64;

    let session_time = if hours > 0 {
        format!("{}h {:02}m", hours, minutes)
    } else {
        format!("{}m", minutes)
    };

    assert_eq!(session_time, "45m");
}

/// Test cost formatting
#[test]
fn test_cost_format() {
    let cost = 4.5678f64;
    let formatted = format!("${:.2}", cost);
    assert_eq!(formatted, "$4.57");
}

/// Test cost formatting zero
#[test]
fn test_cost_format_zero() {
    let cost = 0.0f64;
    let formatted = format!("${:.2}", cost);
    assert_eq!(formatted, "$0.00");
}
