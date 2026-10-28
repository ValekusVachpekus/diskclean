//! Shared spacing, radius, and type-scale constants.

pub(crate) const SPACE_2: f32 = 8.0;
pub(crate) const SPACE_3: f32 = 12.0;

pub(crate) const RADIUS_CARD: u8 = 8;
pub(crate) const RADIUS_CONTROL: u8 = 6;

pub(crate) const FONT_SMALL: f32 = 11.5;
pub(crate) const FONT_BODY: f32 = 13.0;
pub(crate) const FONT_DISPLAY: f32 = 20.0;

pub(crate) const ROW_HEIGHT: f32 = 30.0;
pub(crate) const HEADER_HEIGHT: f32 = 32.0;
pub(crate) const NAV_WIDTH: f32 = 236.0;
pub(crate) const INSPECTOR_WIDTH: f32 = 300.0;
pub(crate) const STATUS_BAR_HEIGHT: f32 = 26.0;

// Responsive shell budgets in logical pixels; resizing must retain table space.
pub(crate) const NAV_RAIL_WIDTH: f32 = 60.0;
pub(crate) const NAV_COMPACT_BREAKPOINT: f32 = 1_000.0;
pub(crate) const COMMAND_COMPACT_BREAKPOINT: f32 = 760.0;
pub(crate) const WORKSPACE_MIN_WIDTH: f32 = 560.0;
