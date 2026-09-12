use whisperpilot_lib::bubble_window::{
    clamp_top_left, classify_gesture, BubbleGesture, LogicalPoint, LogicalRect, LogicalSize,
};

#[test]
fn movement_at_threshold_is_a_click_and_above_threshold_is_drag_only() {
    let origin = LogicalPoint::new(10.0, 20.0);

    assert_eq!(
        classify_gesture(origin, LogicalPoint::new(13.0, 24.0), 5.0),
        BubbleGesture::Click
    );
    assert_eq!(
        classify_gesture(origin, LogicalPoint::new(14.0, 24.0), 5.0),
        BubbleGesture::Drag
    );
}

#[test]
fn restore_position_is_clamped_to_the_monitor_that_contains_the_bubble() {
    let displays = [
        LogicalRect::new(0.0, 24.0, 1512.0, 958.0),
        LogicalRect::new(1512.0, 0.0, 1920.0, 1080.0),
    ];
    let main = LogicalSize::new(1000.0, 760.0);

    assert_eq!(
        clamp_top_left(LogicalPoint::new(3300.0, 1000.0), main, &displays),
        LogicalPoint::new(2432.0, 320.0)
    );
}

#[test]
fn removed_monitor_falls_back_to_a_visible_primary_position() {
    let primary = LogicalRect::new(0.0, 24.0, 1512.0, 958.0);

    assert_eq!(
        clamp_top_left(
            LogicalPoint::new(-4000.0, 800.0),
            LogicalSize::new(1000.0, 760.0),
            &[primary]
        ),
        LogicalPoint::new(0.0, 222.0)
    );
}
