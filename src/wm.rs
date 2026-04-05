use ratatui::layout::Direction;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplitDirection {
    Horizontal,
    Vertical,
}

impl Into<Direction> for SplitDirection {
    fn into(self) -> Direction {
        match self {
            SplitDirection::Horizontal => Direction::Horizontal,
            SplitDirection::Vertical => Direction::Vertical,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavDirection {
    Up,
    Down,
    Left,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PaneId {
    Positions,
    Accounts,
    History,
    Markets,
    Live,
    Props,
    Chart,
    Intel,
    Matcher,
    Stats,
    Alerts,
    Calculator,
    Recorder,
    Observability,
}

impl PaneId {
    pub fn title(self) -> &'static str {
        match self {
            Self::Positions => "Live Orders",
            Self::Accounts => "Accounts",
            Self::History => "History",
            Self::Markets => "Markets",
            Self::Live => "Live",
            Self::Props => "Props",
            Self::Chart => "Chart",
            Self::Intel => "Opportunities",
            Self::Matcher => "Matcher",
            Self::Stats => "Stats",
            Self::Alerts => "Alerts",
            Self::Calculator => "Calc",
            Self::Recorder => "Recorder",
            Self::Observability => "Observability",
        }
    }
}

#[derive(Debug, Clone)]
pub enum LayoutNode {
    Pane(PaneId),
    Split {
        direction: SplitDirection,
        ratios: Vec<u16>, // Percentages
        children: Vec<LayoutNode>,
    },
}

#[derive(Debug, Clone)]
pub struct Workspace {
    pub name: String,
    pub root: LayoutNode,
    pub minimized: Vec<PaneId>,
    pub emphasized_pane: Option<PaneId>,
}

impl Workspace {
    fn first_pane(&self) -> Option<PaneId> {
        self.root.first_pane()
    }

    pub fn contains_pane(&self, pane: PaneId) -> bool {
        self.root.contains_pane(pane) || self.minimized.contains(&pane)
    }

    pub fn is_minimized(&self, pane: PaneId) -> bool {
        self.minimized.contains(&pane)
    }

    fn pane_rects(&self) -> Vec<(PaneId, PaneRect)> {
        let mut panes = Vec::new();
        self.root
            .collect_panes(PaneRect::CANVAS, self.emphasized_pane, &mut panes);
        panes
    }

    fn visible_pane_count(&self) -> usize {
        self.pane_rects().len()
    }
}

#[derive(Debug, Clone)]
pub struct WindowManager {
    pub workspaces: Vec<Workspace>,
    pub active_workspace: usize,
    pub active_pane: Option<PaneId>,
    pub maximized_pane: Option<PaneId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PaneRect {
    x: i32,
    y: i32,
    width: i32,
    height: i32,
}

impl PaneRect {
    const CANVAS: Self = Self {
        x: 0,
        y: 0,
        width: 1200,
        height: 1200,
    };

    fn center_x(self) -> i32 {
        self.x + (self.width / 2)
    }

    fn center_y(self) -> i32 {
        self.y + (self.height / 2)
    }

    fn primary_distance(self, other: Self, direction: NavDirection) -> i32 {
        match direction {
            NavDirection::Left => self.x - (other.x + other.width),
            NavDirection::Right => other.x - (self.x + self.width),
            NavDirection::Up => self.y - (other.y + other.height),
            NavDirection::Down => other.y - (self.y + self.height),
        }
        .max(0)
    }

    fn secondary_distance(self, other: Self, direction: NavDirection) -> i32 {
        match direction {
            NavDirection::Left | NavDirection::Right => (self.center_y() - other.center_y()).abs(),
            NavDirection::Up | NavDirection::Down => (self.center_x() - other.center_x()).abs(),
        }
    }

    fn orthogonal_overlap(self, other: Self, direction: NavDirection) -> i32 {
        let (self_start, self_end, other_start, other_end) = match direction {
            NavDirection::Left | NavDirection::Right => (
                self.y,
                self.y + self.height,
                other.y,
                other.y + other.height,
            ),
            NavDirection::Up | NavDirection::Down => {
                (self.x, self.x + self.width, other.x, other.x + other.width)
            }
        };

        (self_end.min(other_end) - self_start.max(other_start)).max(0)
    }

    fn is_in_direction(self, other: Self, direction: NavDirection) -> bool {
        match direction {
            NavDirection::Left => other.center_x() < self.center_x(),
            NavDirection::Right => other.center_x() > self.center_x(),
            NavDirection::Up => other.center_y() < self.center_y(),
            NavDirection::Down => other.center_y() > self.center_y(),
        }
    }
}

impl LayoutNode {
    fn first_pane(&self) -> Option<PaneId> {
        match self {
            Self::Pane(pane) => Some(*pane),
            Self::Split { children, .. } => children.iter().find_map(Self::first_pane),
        }
    }

    fn contains_pane(&self, target: PaneId) -> bool {
        match self {
            Self::Pane(pane) => *pane == target,
            Self::Split { children, .. } => {
                children.iter().any(|child| child.contains_pane(target))
            }
        }
    }

    fn collect_panes(
        &self,
        rect: PaneRect,
        emphasized_pane: Option<PaneId>,
        out: &mut Vec<(PaneId, PaneRect)>,
    ) {
        match self {
            Self::Pane(pane) => out.push((*pane, rect)),
            Self::Split {
                direction,
                ratios,
                children,
            } => {
                let display_ratios = effective_ratios(ratios, children, emphasized_pane);
                let total = display_ratios
                    .iter()
                    .copied()
                    .map(i32::from)
                    .sum::<i32>()
                    .max(1);
                let mut cursor_x = rect.x;
                let mut cursor_y = rect.y;

                for (index, child) in children.iter().enumerate() {
                    let weight = display_ratios
                        .get(index)
                        .copied()
                        .map(i32::from)
                        .unwrap_or(0);
                    let is_last = index + 1 == children.len();
                    let child_rect = match direction {
                        SplitDirection::Horizontal => {
                            let remaining = (rect.x + rect.width) - cursor_x;
                            let width = if is_last {
                                remaining
                            } else {
                                (rect.width * weight) / total
                            };
                            let child_rect = PaneRect {
                                x: cursor_x,
                                y: rect.y,
                                width: width.max(0),
                                height: rect.height,
                            };
                            cursor_x += width;
                            child_rect
                        }
                        SplitDirection::Vertical => {
                            let remaining = (rect.y + rect.height) - cursor_y;
                            let height = if is_last {
                                remaining
                            } else {
                                (rect.height * weight) / total
                            };
                            let child_rect = PaneRect {
                                x: rect.x,
                                y: cursor_y,
                                width: rect.width,
                                height: height.max(0),
                            };
                            cursor_y += height;
                            child_rect
                        }
                    };

                    child.collect_panes(child_rect, emphasized_pane, out);
                }
            }
        }
    }

    fn remove_pane(&self, target: PaneId) -> Option<Self> {
        match self {
            Self::Pane(pane) => (*pane != target).then_some(Self::Pane(*pane)),
            Self::Split {
                direction,
                ratios,
                children,
            } => {
                let mut remaining_children = Vec::new();
                let mut remaining_ratios = Vec::new();

                for (index, child) in children.iter().enumerate() {
                    if let Some(updated_child) = child.remove_pane(target) {
                        remaining_children.push(updated_child);
                        remaining_ratios.push(ratios.get(index).copied().unwrap_or(0));
                    }
                }

                match remaining_children.len() {
                    0 => None,
                    1 => remaining_children.into_iter().next(),
                    _ => Some(Self::Split {
                        direction: *direction,
                        ratios: normalize_ratios(&remaining_ratios),
                        children: remaining_children,
                    }),
                }
            }
        }
    }
}

fn pane(pane_id: PaneId) -> LayoutNode {
    LayoutNode::Pane(pane_id)
}

fn split(direction: SplitDirection, ratios: Vec<u16>, children: Vec<LayoutNode>) -> LayoutNode {
    LayoutNode::Split {
        direction,
        ratios,
        children,
    }
}

fn normalize_ratios(ratios: &[u16]) -> Vec<u16> {
    if ratios.is_empty() {
        return Vec::new();
    }

    let total = ratios.iter().copied().map(u32::from).sum::<u32>().max(1);
    let mut normalized = Vec::with_capacity(ratios.len());
    let mut assigned = 0u16;

    for (index, ratio) in ratios.iter().copied().enumerate() {
        if index + 1 == ratios.len() {
            normalized.push(100u16.saturating_sub(assigned));
        } else {
            let scaled = ((u32::from(ratio) * 100) / total) as u16;
            normalized.push(scaled);
            assigned = assigned.saturating_add(scaled);
        }
    }

    normalized
}

pub(crate) fn effective_ratios(
    ratios: &[u16],
    children: &[LayoutNode],
    emphasized_pane: Option<PaneId>,
) -> Vec<u16> {
    let base = normalize_ratios(ratios);
    let Some(emphasized_pane) = emphasized_pane else {
        return base;
    };
    let Some(index) = children
        .iter()
        .position(|child| child.contains_pane(emphasized_pane))
    else {
        return base;
    };
    if children.len() < 2 {
        return base;
    }

    let baseline_ratio = base.get(index).copied().unwrap_or_default();
    let emphasis_ratio = match children.len() {
        2 => baseline_ratio.saturating_add(12).clamp(68, 82),
        3 => baseline_ratio.saturating_add(8).clamp(56, 72),
        _ => baseline_ratio.saturating_add(6).clamp(50, 60),
    };
    if baseline_ratio >= emphasis_ratio {
        return base;
    }

    let remainder = 100u16.saturating_sub(emphasis_ratio);
    let remaining_total = base
        .iter()
        .enumerate()
        .filter(|(child_index, _)| *child_index != index)
        .map(|(_, ratio)| u32::from(*ratio))
        .sum::<u32>()
        .max(1);
    let last_other = base
        .iter()
        .enumerate()
        .rfind(|(child_index, _)| *child_index != index)
        .map(|(child_index, _)| child_index)
        .unwrap_or(index);

    let mut adjusted = vec![0u16; base.len()];
    adjusted[index] = emphasis_ratio;
    let mut assigned = emphasis_ratio;
    for (child_index, ratio) in base.iter().copied().enumerate() {
        if child_index == index {
            continue;
        }
        let scaled = if child_index == last_other {
            100u16.saturating_sub(assigned)
        } else {
            ((u32::from(ratio) * u32::from(remainder)) / remaining_total) as u16
        };
        adjusted[child_index] = scaled;
        assigned = assigned.saturating_add(scaled);
    }
    adjusted
}

impl WindowManager {
    pub fn new() -> Self {
        Self {
            workspaces: vec![
                // Fallback / initial workspace, can be replaced by predefined ones
                Workspace {
                    name: "Default".to_string(),
                    root: LayoutNode::Pane(PaneId::Positions),
                    minimized: Vec::new(),
                    emphasized_pane: None,
                },
            ],
            active_workspace: 0,
            active_pane: Some(PaneId::Positions),
            maximized_pane: None,
        }
    }

    pub fn set_predefined_workspaces(&mut self) {
        self.workspaces = vec![
            Workspace {
                name: "Ledger".to_string(),
                root: split(
                    SplitDirection::Horizontal,
                    vec![22, 50, 28],
                    vec![
                        split(
                            SplitDirection::Vertical,
                            vec![60, 40],
                            vec![pane(PaneId::Intel), pane(PaneId::Stats)],
                        ),
                        split(
                            SplitDirection::Vertical,
                            vec![70, 30],
                            vec![pane(PaneId::Chart), pane(PaneId::History)],
                        ),
                        pane(PaneId::Positions),
                    ],
                ),
                minimized: vec![PaneId::Calculator],
                emphasized_pane: None,
            },
            Workspace {
                name: "Markets".to_string(),
                root: split(
                    SplitDirection::Vertical,
                    vec![58, 42],
                    vec![pane(PaneId::Chart), pane(PaneId::Markets)],
                ),
                minimized: vec![PaneId::Matcher, PaneId::Live, PaneId::Props],
                emphasized_pane: None,
            },
            Workspace {
                name: "Control".to_string(),
                root: split(
                    SplitDirection::Horizontal,
                    vec![34, 66],
                    vec![
                        split(
                            SplitDirection::Vertical,
                            vec![46, 54],
                            vec![pane(PaneId::Accounts), pane(PaneId::Recorder)],
                        ),
                        pane(PaneId::Observability),
                    ],
                ),
                minimized: vec![PaneId::Alerts],
                emphasized_pane: None,
            },
        ];
        self.active_workspace = 0;
        self.active_pane = Some(PaneId::Positions);
        self.maximized_pane = None;
    }

    pub fn current_workspace(&self) -> &Workspace {
        &self.workspaces[self.active_workspace]
    }

    fn current_workspace_mut(&mut self) -> &mut Workspace {
        &mut self.workspaces[self.active_workspace]
    }

    pub fn can_minimize_pane(&self, pane: PaneId) -> bool {
        let workspace = self.current_workspace();
        workspace.root.contains_pane(pane) && workspace.visible_pane_count() > 1
    }

    pub fn minimize_pane(&mut self, pane: PaneId) -> bool {
        if !self.can_minimize_pane(pane) {
            return false;
        }

        let Some(updated_root) = self.current_workspace().root.remove_pane(pane) else {
            return false;
        };

        let was_active = self.active_pane == Some(pane);
        let was_maximized = self.maximized_pane == Some(pane);
        let next_active;
        let workspace = self.current_workspace_mut();
        workspace.root = updated_root;
        if workspace.emphasized_pane == Some(pane) {
            workspace.emphasized_pane = None;
        }
        if !workspace.minimized.contains(&pane) {
            workspace.minimized.push(pane);
        }
        next_active = if was_active {
            workspace.first_pane()
        } else {
            None
        };

        if was_active {
            self.active_pane = next_active;
        }
        if was_maximized {
            self.maximized_pane = None;
        }

        true
    }

    pub fn restore_minimized_pane(&mut self, pane: PaneId) -> bool {
        if !self.current_workspace().is_minimized(pane) {
            return false;
        }
        self.active_pane = Some(pane);
        self.maximized_pane = Some(pane);
        true
    }

    pub fn toggle_maximize(&mut self) {
        if self.maximized_pane.is_some() {
            self.maximized_pane = None;
        } else if let Some(active) = self.active_pane {
            self.maximized_pane = Some(active);
        }
    }

    pub fn switch_workspace(&mut self, index: usize) -> Option<PaneId> {
        if index < self.workspaces.len() {
            self.active_workspace = index;
            self.maximized_pane = None;
            self.active_pane = self.workspaces[index].first_pane();
        }

        self.active_pane
    }

    pub fn workspace_index_for_pane(&self, pane: PaneId) -> Option<usize> {
        self.workspaces
            .iter()
            .position(|workspace| workspace.contains_pane(pane))
    }

    pub fn focus_pane(&mut self, pane: PaneId) -> bool {
        if self.current_workspace().contains_pane(pane) {
            self.active_pane = Some(pane);
            true
        } else {
            false
        }
    }

    pub fn toggle_pane_emphasis(&mut self, pane: PaneId) -> bool {
        if !self.current_workspace().root.contains_pane(pane) {
            return false;
        }

        let workspace = self.current_workspace_mut();
        workspace.emphasized_pane = if workspace.emphasized_pane == Some(pane) {
            None
        } else {
            Some(pane)
        };
        workspace.emphasized_pane == Some(pane)
    }

    pub fn focus_neighbor(&mut self, direction: NavDirection) -> Option<PaneId> {
        let current = self.active_pane?;
        let panes = self.current_workspace().pane_rects();
        let current_rect =
            panes
                .iter()
                .find_map(|(pane, rect)| if *pane == current { Some(*rect) } else { None })?;

        let next = panes
            .iter()
            .filter(|(pane, rect)| {
                *pane != current && current_rect.is_in_direction(*rect, direction)
            })
            .min_by_key(|(_, rect)| {
                (
                    current_rect.primary_distance(*rect, direction),
                    -current_rect.orthogonal_overlap(*rect, direction),
                    current_rect.secondary_distance(*rect, direction),
                )
            })
            .map(|(pane, _)| *pane)?;

        self.active_pane = Some(next);
        Some(next)
    }
}

impl Default for WindowManager {
    fn default() -> Self {
        let mut wm = Self::new();
        wm.set_predefined_workspaces();
        wm
    }
}

#[cfg(test)]
mod tests {
    use super::{PaneId, WindowManager};

    #[test]
    fn predefined_workspaces_keep_minimized_panes_out_of_live_layout() {
        let mut wm = WindowManager::default();

        let ledger_panes = wm.current_workspace().pane_rects();
        assert!(ledger_panes
            .iter()
            .all(|(pane, _)| *pane != PaneId::Calculator));
        assert!(ledger_panes
            .iter()
            .any(|(pane, _)| *pane == PaneId::History));

        wm.switch_workspace(1);
        let market_panes = wm.current_workspace().pane_rects();
        assert!(market_panes
            .iter()
            .any(|(pane, _)| matches!(pane, PaneId::Markets | PaneId::Live | PaneId::Props)));
        assert!(market_panes
            .iter()
            .all(|(pane, _)| !matches!(pane, PaneId::Matcher)));

        wm.switch_workspace(2);
        let control_panes = wm.current_workspace().pane_rects();
        assert!(control_panes
            .iter()
            .all(|(pane, _)| *pane != PaneId::Alerts));

        wm.switch_workspace(3);
        let stack_panes = wm.current_workspace().pane_rects();
        assert!(stack_panes.iter().any(|(pane, _)| matches!(
            pane,
            PaneId::Recorder | PaneId::Alerts | PaneId::Observability | PaneId::Matcher
        )));
    }

    #[test]
    fn minimizing_active_visible_pane_moves_it_to_strip() {
        let mut wm = WindowManager::default();

        assert_eq!(wm.active_pane, Some(PaneId::Positions));
        assert!(wm.minimize_pane(PaneId::Positions));
        assert!(wm.current_workspace().is_minimized(PaneId::Positions));
        assert!(!wm.current_workspace().root.contains_pane(PaneId::Positions));
        assert_ne!(wm.active_pane, Some(PaneId::Positions));
    }

    #[test]
    fn emphasized_pane_expands_its_visible_rect() {
        let mut wm = WindowManager::default();

        let baseline = wm
            .current_workspace()
            .pane_rects()
            .into_iter()
            .find_map(|(pane, rect)| (pane == PaneId::Chart).then_some(rect))
            .expect("chart rect");

        assert!(wm.toggle_pane_emphasis(PaneId::Chart));

        let emphasized = wm
            .current_workspace()
            .pane_rects()
            .into_iter()
            .find_map(|(pane, rect)| (pane == PaneId::Chart).then_some(rect))
            .expect("emphasized chart rect");

        assert!(emphasized.width >= baseline.width);
        assert!(emphasized.height > baseline.height);

        assert!(!wm.toggle_pane_emphasis(PaneId::Chart));
    }
}
