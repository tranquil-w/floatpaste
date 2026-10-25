//! 速贴与搜索列表、动作菜单共用的循环导航，不依赖 GUI。
pub fn next_navigation_index(count: usize, current: usize, up: bool) -> usize {
    if count == 0 {
        return 0;
    }
    if up {
        (current + count - 1) % count
    } else {
        (current + 1) % count
    }
}

#[cfg(test)]
mod tests {
    use super::next_navigation_index;

    #[test]
    fn navigation_wraps_in_both_directions() {
        assert_eq!(next_navigation_index(5, 0, true), 4);
        assert_eq!(next_navigation_index(5, 4, true), 3);
        assert_eq!(next_navigation_index(5, 4, false), 0);
        assert_eq!(next_navigation_index(1, 0, true), 0);
        assert_eq!(next_navigation_index(0, 0, false), 0);
    }
}
