//! Geometry tokens, in logical px at zoom 1. The reference was captured at 2× (1812×1344 device
//! px for a 906×672 window), so every value here is half of what is measured on the video frames.

#[derive(Debug, Clone, PartialEq)]
pub struct Metrics {
    /// Iosevka's advance is 0.5em → 10 px per char.
    pub font_size: f32,
    pub char_w: f32,
    pub row_h: f32,
    /// Text box offset inside its row (aligns baselines with the reference).
    pub text_y: f32,
    /// Longer names are cut to max_chars-1 cells + "…" (except the selected one).
    pub max_chars: usize,
    /// Every name occupies one extra trailing cell (except a selected name past max_chars).
    pub cell_pad: usize,
    /// Between columns, after the trailing cell.
    pub path_gap: f32,
    /// Selection column → first preview column (room for connector lanes).
    pub preview_gap: f32,
    /// Blank rows between stacked preview groups.
    pub group_gap_rows: f32,
    /// Where connectors meet a row, from the row top.
    pub connector_y: f32,
    /// Elbowed connectors enter a group at its edge, shifted toward the parent.
    pub enter_offset: f32,
    /// Gap between a name's last glyph and its outgoing connector.
    pub conn_gap_l: f32,
    /// Gap between an incoming connector and the child's first glyph.
    pub conn_gap_r: f32,
    /// Vertical run next to the parent (when nothing is in the way).
    pub near_parent: f32,
    /// Vertical run next to the child column.
    pub near_child: f32,
    /// Spacing between parallel vertical runs.
    pub lane_gap: f32,
    /// Shortest horizontal stub before a vertical run.
    pub min_run: f32,
    /// A landing connector stops this far above/below the group's glyphs.
    pub land_gap: f32,
    /// Length of the landing hook.
    pub hook: f32,
    /// Elbow rounding.
    pub radius: f32,
    pub stroke_w: f32,
    pub path_stroke_w: f32,
    /// Off-screen connector stub.
    pub dot_r: f32,
    pub caret_w: f32,
    pub caret_x: f32,
    /// The reference centres the tree ~45 px right of the window centre.
    pub camera_bias_x: f32,
    /// Selection row top, as a fraction of the viewport height.
    pub focus_y: f32,
    /// Animation length in ms; the reference snaps (0), the spec asks for interpolation.
    pub duration_ms: f32,
    pub overscan: f32,
    /// Rows materialized above/below the focus row (virtualized layout).
    pub window_rows: f32,
}

impl Default for Metrics {
    fn default() -> Self {
        Metrics {
            font_size: 20.0,
            char_w: 10.0,
            row_h: 16.0,
            text_y: -1.5,
            max_chars: 25,
            cell_pad: 1,
            path_gap: 0.0,
            preview_gap: 9.5,
            group_gap_rows: 1.0,
            connector_y: 7.25,
            enter_offset: 5.25,
            conn_gap_l: 3.0,
            conn_gap_r: 2.0,
            near_parent: 5.0,
            near_child: 8.0,
            lane_gap: 3.0,
            min_run: 3.0,
            land_gap: 2.0,
            hook: 5.0,
            radius: 5.0,
            stroke_w: 1.5,
            path_stroke_w: 1.5,
            dot_r: 1.5,
            caret_w: 10.5,
            caret_x: -0.5,
            camera_bias_x: 45.0,
            focus_y: 0.5,
            duration_ms: 240.0,
            overscan: 64.0,
            window_rows: 120.0,
        }
    }
}
