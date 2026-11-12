use crate::DndMode;

pub const INTERRUPTION_FILTER_ALL: i32 = 1;
pub const INTERRUPTION_FILTER_PRIORITY: i32 = 2;
pub const INTERRUPTION_FILTER_NONE: i32 = 3;
pub const INTERRUPTION_FILTER_ALARMS: i32 = 4;

pub fn target_filter(mode: DndMode) -> Option<i32> {
    match mode {
        DndMode::Off => None,
        DndMode::Priority => Some(INTERRUPTION_FILTER_PRIORITY),
        DndMode::Alarms => Some(INTERRUPTION_FILTER_ALARMS),
    }
}

pub fn should_restore(engaged: bool, current_filter: i32, set_filter: i32) -> bool {
    engaged && current_filter == set_filter
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_modes_to_android_interruption_filters() {
        assert_eq!(target_filter(DndMode::Off), None);
        assert_eq!(
            target_filter(DndMode::Priority),
            Some(INTERRUPTION_FILTER_PRIORITY)
        );
        assert_eq!(
            target_filter(DndMode::Alarms),
            Some(INTERRUPTION_FILTER_ALARMS)
        );
        assert_ne!(INTERRUPTION_FILTER_ALL, INTERRUPTION_FILTER_NONE);
    }

    #[test]
    fn restores_only_if_the_student_has_not_changed_the_filter() {
        assert!(should_restore(
            true,
            INTERRUPTION_FILTER_PRIORITY,
            INTERRUPTION_FILTER_PRIORITY
        ));
        assert!(!should_restore(
            true,
            INTERRUPTION_FILTER_ALARMS,
            INTERRUPTION_FILTER_PRIORITY
        ));
        assert!(!should_restore(
            false,
            INTERRUPTION_FILTER_PRIORITY,
            INTERRUPTION_FILTER_PRIORITY
        ));
    }
}
