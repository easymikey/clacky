use rstest::rstest;
use unicode_segmentation::UnicodeSegmentation as _;
use zeroize::Zeroizing;

use crate::{
    core::ask::Next,
    prompts::text::Field,
    render::draw::Echo,
    values::line::{CAPACITY, CharFilter, Csi, Key, TypedAnswer, ss3_key, with_room},
};

fn field(filter: CharFilter) -> Field {
    Field {
        filter,
        ..Field::new("", Echo::Shown)
    }
}

fn typed(text: &str) -> impl Iterator<Item = Key> + '_ {
    text.chars().map(|c| match c {
        '\n' => Key::Enter,
        c => Key::Char(c),
    })
}

#[derive(Debug, PartialEq, Eq)]
enum End {
    Submitted(String),
    Typing(String),
    Cancelled,
}

fn run(filter: CharFilter, strokes: impl IntoIterator<Item = Key>) -> End {
    let field = field(filter);
    let last =
        strokes.into_iter().try_fold(field.start(), |line, stroke| {
            match field.update(line, stroke) {
                Next::Editing(line) | Next::Refuse(line, _) => Ok(line),
                Next::Submit(line) => Err(End::Submitted(line.text().to_owned())),
                Next::Quit(..) => Err(End::Cancelled),
            }
        });
    last.map_or_else(|end| end, |line| End::Typing(line.text().to_owned()))
}

fn typing(filter: CharFilter, strokes: impl IntoIterator<Item = Key>) -> TypedAnswer {
    let field = field(filter);
    strokes.into_iter().fold(field.start(), |line, stroke| {
        match field.update(line, stroke) {
            Next::Editing(line) | Next::Refuse(line, _) => line,
            Next::Submit(_) | Next::Quit(..) => field.start(),
        }
    })
}

#[test]
fn enter_submits_trimmed_text() {
    assert_eq!(
        run(CharFilter::Printable, typed(" you@example.com \n")),
        End::Submitted("you@example.com".into())
    );
}

#[rstest]
#[case::ascii("abc", "ab")]
#[case::cyrillic("код", "ко")]
#[case::combining("cafe\u{301}", "caf")]
#[case::family_emoji("a👨‍👩‍👧", "a")]
#[case::flag("x🇺🇦", "x")]
fn backspace_removes_one_grapheme(#[case] text: &str, #[case] left: &str) {
    let strokes = typed(text).chain([Key::Backspace]);
    assert_eq!(
        run(CharFilter::Printable, strokes),
        End::Typing(left.into())
    );
}

#[test]
fn backspace_on_empty_line_does_nothing() {
    assert_eq!(
        run(CharFilter::NoSpaces, [Key::Backspace, Key::Backspace]),
        End::Typing(String::new())
    );
}

#[test]
fn ctrl_u_clears_and_ctrl_w_removes_a_word() {
    assert_eq!(
        run(
            CharFilter::Printable,
            typed("abc").chain([Key::ClearBefore])
        ),
        End::Typing(String::new())
    );
    assert_eq!(
        run(
            CharFilter::Printable,
            typed("one two  ").chain([Key::DeleteWord])
        ),
        End::Typing("one ".into())
    );
    assert_eq!(
        run(
            CharFilter::Printable,
            typed("single").chain([Key::DeleteWord])
        ),
        End::Typing(String::new())
    );
}

#[test]
fn code_drops_whitespace_of_a_paste() {
    assert_eq!(
        run(CharFilter::NoSpaces, typed(" 123 456\t\n")),
        End::Submitted("123456".into())
    );
}

#[test]
fn email_keeps_inner_spaces_and_trims_edges() {
    assert_eq!(
        run(CharFilter::Printable, typed("  a b@c.d  \n")),
        End::Submitted("a b@c.d".into())
    );
}

#[test]
fn pasted_newline_submits_the_first_line() {
    assert_eq!(
        run(CharFilter::NoSpaces, typed("123456\n999")),
        End::Submitted("123456".into())
    );
}

#[rstest]
#[case::ctrl_c(Key::Interrupt)]
#[case::esc(Key::Cancel)]
#[case::ctrl_d_on_empty(Key::EndOfInput)]
fn cancel_strokes(#[case] stroke: Key) {
    assert_eq!(run(CharFilter::NoSpaces, [stroke]), End::Cancelled);
}

#[test]
fn ctrl_d_on_a_typed_line_is_ignored() {
    assert_eq!(
        run(CharFilter::NoSpaces, typed("12").chain([Key::EndOfInput])),
        End::Typing("12".into())
    );
}

#[test]
fn keys_without_an_effect_leave_no_trace() {
    let strokes = typed("1")
        .chain([Key::Ignored, Key::Char('\u{7}')])
        .chain(typed("2"));
    assert_eq!(
        run(CharFilter::Printable, strokes),
        End::Typing("12".into())
    );
}

#[test]
fn mask_length_counts_graphemes() {
    let line = typing(CharFilter::NoSpaces, typed("12👨‍👩‍👧"));
    assert_eq!(line.text().graphemes(true).count(), 3);
}

#[test]
fn answer_longer_than_the_reserve_survives() {
    let long = "x".repeat(CAPACITY * 3 + 1);
    let strokes = typed(&long).chain([Key::Enter]);
    assert_eq!(
        run(CharFilter::Printable, strokes),
        End::Submitted(long.clone())
    );
    assert!(with_room(Zeroizing::new(String::new()), 4).capacity() >= 4);
}

#[test]
fn prefilled_text_is_filtered_and_the_cursor_sits_after_it() {
    let filled = TypedAnswer::new(CharFilter::NoSpaces).prefilled("12 3");
    assert_eq!(filled.text(), "123");
    assert_eq!(filled.graphemes_before(), 3);
    let end = field(CharFilter::NoSpaces).update(filled, Key::Char('4'));
    assert!(matches!(end, Next::Editing(line) if line.text() == "1234"));
}

fn after_keys(field: CharFilter, text: &str, keys: &[Key]) -> End {
    run(field, typed(text).chain(keys.iter().copied()))
}

#[rstest]
#[case::left_then_type("ac", &[Key::Left, Key::Char('b')], "abc")]
#[case::home_then_type("bc", &[Key::Home, Key::Char('a')], "abc")]
#[case::end_after_home("ab", &[Key::Home, Key::End, Key::Char('c')], "abc")]
#[case::right_at_end("ab", &[Key::Right, Key::Char('c')], "abc")]
#[case::left_at_start("b", &[Key::Home, Key::Left, Key::Char('a')], "ab")]
#[case::backspace_in_middle("abxc", &[Key::Left, Key::Backspace], "abc")]
#[case::backspace_at_start("abc", &[Key::Home, Key::Backspace], "abc")]
#[case::delete_in_middle("abxc", &[Key::Left, Key::Left, Key::Delete], "abc")]
#[case::delete_at_end("abc", &[Key::Delete], "abc")]
#[case::ctrl_u_before_cursor("abc", &[Key::Left, Key::ClearBefore], "c")]
#[case::ctrl_k_after_cursor("abc", &[Key::Left, Key::Left, Key::ClearAfter], "a")]
#[case::ctrl_w_before_cursor("one two three", &[Key::WordLeft, Key::DeleteWord], "one three")]
#[case::cyrillic_left("кд", &[Key::Left, Key::Char('о')], "код")]
#[case::family_left("a👨‍👩‍👧", &[Key::Left, Key::Char('b')], "ab👨‍👩‍👧")]
#[case::flag_left("🇺🇦🇫🇷", &[Key::Left, Key::Backspace], "🇫🇷")]
#[case::combining_delete("cafe\u{301}!", &[Key::Left, Key::Left, Key::Delete], "caf!")]
#[case::up_down_tab_ignored("ab", &[Key::Up, Key::Left, Key::Down, Key::Tab, Key::Char('x')], "axb")]
fn editing_at_the_cursor(
    #[case] text: &str,
    #[case] keys: &[Key],
    #[case] result: &str,
) {
    assert_eq!(
        after_keys(CharFilter::Printable, text, keys),
        End::Typing(result.into())
    );
}

#[rstest]
#[case::word_left("mock@example.com", &[Key::WordLeft, Key::Char('x')], "mock@example.xcom")]
#[case::two_words_left("mock@example.com", &[Key::WordLeft, Key::WordLeft, Key::Char('x')], "mock@xexample.com")]
#[case::word_right("mock@example.com", &[Key::Home, Key::WordRight, Key::Char('x')], "mockx@example.com")]
#[case::word_right_past_separator("ab  cd", &[Key::Home, Key::WordRight, Key::WordRight, Key::Char('x')], "ab  cdx")]
#[case::word_left_at_start("ab", &[Key::Home, Key::WordLeft, Key::Char('x')], "xab")]
fn moving_by_words(#[case] text: &str, #[case] keys: &[Key], #[case] result: &str) {
    assert_eq!(
        after_keys(CharFilter::Printable, text, keys),
        End::Typing(result.into())
    );
}

#[rstest]
#[case::ctrl_left("1;5", 'D', "|a b")]
#[case::alt_left("1;3", 'D', "|a b")]
#[case::shift_left("1;2", 'D', "a| b")]
#[case::alt_right("1;3", 'C', "a b|")]
#[case::home_tilde("1", '~', "|a b")]
#[case::end_tilde("4", '~', "a b|")]
#[case::delete_tilde("3", '~', "a |")]
#[case::unknown_tilde("9", '~', "a |b")]
fn csi_tails_finish_into_keys(
    #[case] digits: &str,
    #[case] last: char,
    #[case] result: &str,
) {
    let csi = digits.chars().fold(Csi::default(), Csi::feed);
    let keys = [Key::Left, csi.key(last), Key::Char('|')];
    assert_eq!(
        after_keys(CharFilter::Printable, "a b", &keys),
        End::Typing(result.into())
    );
}

#[test]
fn ss3_home_and_end() {
    let keys = [ss3_key('H'), Key::Char('x')];
    assert_eq!(
        after_keys(CharFilter::Printable, "ab", &keys),
        End::Typing("xab".into())
    );
}

#[test]
fn code_edits_at_the_cursor() {
    let keys = [Key::Left, Key::Backspace, Key::Enter];
    assert_eq!(
        after_keys(CharFilter::NoSpaces, "123456", &keys),
        End::Submitted("12346".into())
    );
}

#[test]
fn merging_graphemes_keep_the_cursor_on_a_boundary() {
    let strokes = typed("👨👩").chain([Key::Left, Key::Char('\u{200d}')]);
    let line = typing(CharFilter::Printable, strokes);
    assert_eq!(
        (line.text().graphemes(true).count(), line.cursor()),
        (1, line.text().len())
    );
}

mod laws {
    use proptest::prelude::{
        Just,
        ProptestConfig,
        Strategy,
        prop,
        prop_assert,
        prop_assert_eq,
        prop_oneof,
        proptest,
    };
    use unicode_segmentation::UnicodeSegmentation as _;

    use crate::{
        core::ask::Next,
        values::line::{CharFilter, Key, TypedAnswer, tests::field},
    };

    fn stroke() -> impl Strategy<Value = Key> {
        prop_oneof![
            prop::sample::select(vec![
                'a', 'я', ' ', '@', '\u{301}', '\u{200d}', '👨', '🇺', '界'
            ])
            .prop_map(Key::Char),
            Just(Key::Backspace),
            Just(Key::Delete),
            Just(Key::Left),
            Just(Key::Right),
            Just(Key::WordLeft),
            Just(Key::WordRight),
            Just(Key::Home),
            Just(Key::End),
            Just(Key::ClearBefore),
            Just(Key::ClearAfter),
            Just(Key::DeleteWord),
        ]
    }

    fn typing(line: TypedAnswer, stroke: Key) -> TypedAnswer {
        match field(CharFilter::Printable).update(line, stroke) {
            Next::Editing(line) | Next::Refuse(line, _) => line,
            Next::Submit(_) | Next::Quit(..) => TypedAnswer::new(CharFilter::Printable),
        }
    }

    fn on_boundary(line: &TypedAnswer) -> bool {
        line.text()
            .grapheme_indices(true)
            .map(|(at, _)| at)
            .chain([line.text().len()])
            .any(|at| at == line.cursor())
    }

    proptest! {
        #![proptest_config(ProptestConfig { cases: 256, rng_seed: proptest::test_runner::RngSeed::Fixed(0x5eed), ..ProptestConfig::default() })]

        #[test]
        fn cursor_stays_on_a_boundary(strokes in prop::collection::vec(stroke(), 0..40)) {
            let line = strokes.into_iter().fold(TypedAnswer::new(CharFilter::Printable), typing);
            prop_assert!(line.cursor() <= line.text().len());
            prop_assert!(on_boundary(&line), "{:?} at {}", line.text(), line.cursor());
        }

        #[test]
        fn left_then_right_comes_back(text in "[a-zя @.]{1,20}", back in 0usize..20) {
            let line = text.chars().map(Key::Char).fold(TypedAnswer::new(CharFilter::Printable), typing);
            let moved = std::iter::repeat_n(Key::Left, back).fold(line, typing);
            let at = moved.cursor();
            let back_again = typing(typing(moved, Key::Left), Key::Right);
            prop_assert!(at == 0 || back_again.cursor() == at);
        }

        #[test]
        fn home_then_typing_is_a_prefix(text in "[a-z]{0,20}", prefix in "[a-z]{1,10}") {
            let line = text.chars().map(Key::Char).fold(TypedAnswer::new(CharFilter::Printable), typing);
            let line = prefix.chars().map(Key::Char).fold(typing(line, Key::Home), typing);
            prop_assert_eq!(line.text(), format!("{prefix}{text}"));
        }
    }
}
