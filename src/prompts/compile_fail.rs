//! Typestate checks: each block must fail to compile.

/// Kept out of the public docs.
///
/// ```compile_fail
/// use clacky::prelude::*;
/// let _ = group_multiselect("Kind").simulate([Key::Enter]);
/// ```
fn group_multiselect_1() {}

/// Kept out of the public docs.
///
/// ```compile_fail
/// use clacky::prelude::*;
/// let _ = multiline("Notes").initial("- ").placeholder("What changed?");
/// ```
fn multiline_1() {}

/// Kept out of the public docs.
///
/// ```compile_fail
/// use clacky::prelude::*;
/// let _ = multiselect("Kind").simulate([Key::Enter]);
/// ```
fn multiselect_1() {}

/// Kept out of the public docs.
///
/// ```compile_fail
/// use clacky::prelude::*;
/// let _ = select("Kind").hint("recommended");
/// ```
fn select_1() {}

/// Kept out of the public docs.
///
/// ```compile_fail
/// use clacky::prelude::*;
/// let _ = select("Kind").simulate([Key::Enter]);
/// ```
fn select_2() {}

/// Kept out of the public docs.
///
/// ```compile_fail
/// # use clacky::prelude::*;
/// let _ = select_key("Kind").hint("recommended");
/// ```
fn select_key_1() {}

/// Kept out of the public docs.
///
/// ```compile_fail
/// use clacky::prelude::*;
/// let _ = text("Name").placeholder("Ada").initial("Grace");
/// ```
fn text_1() {}
