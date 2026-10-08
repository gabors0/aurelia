//! Layout maths for the poster grid.

/// How many columns fit, and how wide each card is so the row fills exactly.
pub fn columns(available: f32, min_card: f32, gap: f32) -> (usize, f32) {
    let fit = ((available + gap) / (min_card + gap)).floor() as usize;
    let columns = fit.max(2);
    let width = (available - gap * (columns - 1) as f32) / columns as f32;
    (columns, width)
}

/// Whether to fetch the next page: the visible rows reach within two rows of
/// the end of what's loaded, and the server has more.
pub fn needs_more(loaded: usize, total: usize, visible_row_end: usize, columns: usize) -> bool {
    loaded < total && visible_row_end + 2 >= row_count(loaded, columns)
}

pub fn row_count(items: usize, columns: usize) -> usize {
    items.div_ceil(columns.max(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fills_the_row_exactly() {
        let (cols, width) = columns(1344., 172., 20.);
        assert_eq!(cols, 7);
        assert!((width * 7. + 20. * 6. - 1344.).abs() < 0.01);
        assert!(width >= 172.);
    }

    #[test]
    fn narrow_windows_keep_two_columns() {
        let (cols, width) = columns(300., 172., 20.);
        assert_eq!(cols, 2);
        assert!((width - 140.).abs() < 0.01);
    }

    #[test]
    fn paging_triggers_near_the_end() {
        // 100 loaded, 7 per row → 15 rows; viewing rows up to 12 → fetch.
        assert!(needs_more(100, 500, 13, 7));
        assert!(!needs_more(100, 500, 8, 7));
        assert!(!needs_more(500, 500, 72, 7), "everything loaded");
    }

    #[test]
    fn rows_round_up() {
        assert_eq!(row_count(0, 7), 0);
        assert_eq!(row_count(7, 7), 1);
        assert_eq!(row_count(8, 7), 2);
    }
}
