//! LFL v1, "modern airliner" profile: rates per 14 CFR 121.344(f), control
//! inputs at 8 Hz per Appendix M footnote 18. Vertical speed is not recorded;
//! the ground segment derives it from pressure altitude.
//!
//! Append only: a recording stores ids, so existing rows never move.

use crate::SignalDef as S;
use crate::Unit::*;

pub(crate) const SIGNALS: &[S] = &[
    // Flight state
    S::fdr(0, "time.zulu", Seconds, 0.25, "1"),
    S::fdr(1, "alt.pressure", Feet, 1.0, "2"),
    S::fdr(2, "ias", Knots, 1.0, "3"),
    S::fdr(3, "heading.mag", Degrees, 1.0, "4"),
    S::fdr(4, "groundspeed", Knots, 1.0, "34"),
    S::fdr(5, "pos.lat", Degrees, 1.0, "39"),
    S::fdr(6, "pos.lon", Degrees, 1.0, "39"),
    // Acceleration and attitude
    S::fdr(7, "accel.normal", G, 8.0, "5"),
    S::fdr(8, "accel.long", G, 4.0, "11"),
    S::fdr(9, "accel.lat", G, 4.0, "18"),
    S::fdr(10, "pitch", Degrees, 4.0, "6"),
    S::fdr(11, "roll", Degrees, 2.0, "7"),
    S::fdr(12, "aoa", Degrees, 2.0, "32"),
    S::fdr(13, "sideslip", Degrees, 1.0, "70"),
    // Approach
    S::fdr(14, "alt.radio", Feet, 1.0, "26"),
    S::fdr(15, "nav.loc_dev", Dots, 1.0, "27"),
    S::fdr(16, "nav.gs_dev", Dots, 1.0, "28"),
    S::fdr(17, "air_ground", Discrete, 4.0, "31"), // 1 = on ground
    // Configuration
    S::fdr(18, "flap.handle", Index, 0.5, "20"),
    S::fdr(19, "flap.te_angle", Degrees, 0.5, "85"),
    S::fdr(20, "gear.handle", Discrete, 0.25, "36"),
    S::fdr(21, "gear.extended", Percent, 0.25, "36"),
    S::fdr(22, "spoiler.armed", Discrete, 2.0, "87"),
    S::fdr(23, "spoiler.handle", Percent, 2.0, "87"),
    S::fdr(24, "trim.pitch", Degrees, 1.0, "19"),
    // Control inputs
    S::fdr(25, "ctl.pitch", Percent, 8.0, "12a"),
    S::fdr(26, "ctl.roll", Percent, 8.0, "13a"),
    S::fdr(27, "ctl.yaw", Percent, 8.0, "14a"),
    // Engines: one parameter per engine, as on a real frame
    S::fdr(28, "eng1.n1", Percent, 1.0, "9"),
    S::fdr(29, "eng2.n1", Percent, 1.0, "9"),
    S::fdr(30, "eng3.n1", Percent, 1.0, "9"),
    S::fdr(31, "eng4.n1", Percent, 1.0, "9"),
    S::fdr(32, "eng1.throttle", Percent, 1.0, "42"),
    S::fdr(33, "eng2.throttle", Percent, 1.0, "42"),
    S::fdr(34, "eng3.throttle", Percent, 1.0, "42"),
    S::fdr(35, "eng4.throttle", Percent, 1.0, "42"),
    S::fdr(36, "eng1.reverser", Discrete, 1.0, "22"),
    S::fdr(37, "eng2.reverser", Discrete, 1.0, "22"),
    S::fdr(38, "eng3.reverser", Discrete, 1.0, "22"),
    S::fdr(39, "eng4.reverser", Discrete, 1.0, "22"),
    S::fdr(40, "brake.left", Percent, 1.0, "68"),
    S::fdr(41, "brake.right", Percent, 1.0, "68"),
    // Automation
    S::fdr(42, "ap.engaged", Discrete, 1.0, "10"),
    S::fdr(43, "at.engaged", Discrete, 1.0, "25"),
    S::fdr(44, "sel.speed", Knots, 1.0, "49"),
    S::fdr(45, "sel.altitude", Feet, 1.0, "48"),
    S::fdr(46, "sel.vs", FeetPerMinute, 1.0, "51"),
    S::fdr(47, "sel.heading", Degrees, 1.0, "52"),
    S::fdr(48, "stall.warning", Discrete, 1.0, "40"),
    // Environment
    S::fdr(49, "wind.speed", Knots, 0.25, "38"),
    S::fdr(50, "wind.dir", Degrees, 0.25, "38"),
    S::fdr(51, "oat", Celsius, 0.5, "24"),
    S::fdr(52, "baro.setting", Hectopascal, 1.0 / 64.0, "47"),
    S::fdr(53, "dh", Feet, 1.0 / 64.0, "54"),
    // FOQA additions (not in Appendix M)
    S::foqa(54, "weight.gross", Pounds, 0.25),
    S::foqa(55, "fuel.total", Pounds, 0.25),
    // Sim truth, outside the frame
    S::truth(56, "sim.vs", FeetPerMinute, 8.0),
    S::truth(57, "sim.td.vs", FeetPerMinute, 1.0),
    S::truth(58, "sim.td.pitch", Degrees, 1.0),
    S::truth(59, "sim.td.bank", Degrees, 1.0),
    S::truth(60, "sim.rate", Ratio, 1.0),
    S::truth(61, "sim.slew", Discrete, 1.0),
    S::truth(62, "sim.paused", Discrete, 1.0),
    // Appended in 0.11
    S::fdr(63, "heading.true", Degrees, 1.0, "4"), // the true-heading reference of item 4
    S::foqa(64, "mach", Mach, 1.0),
    // Appended in 0.11.3: the course of the tuned localizer, which tells
    // the ground segment which runway end `nav.loc_dev` guides to
    S::foqa(65, "nav.loc_course", Degrees, 1.0),
];
