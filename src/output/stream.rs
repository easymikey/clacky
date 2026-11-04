//! Text that arrives in pieces, drawn on the rail as it comes.

use unicode_segmentation::UnicodeSegmentation as _;

use crate::{
    core::{error::Result, session::Session, settings::OutputMode},
    render::{color::measure_text_width, draw::Tone, wrap::wrap},
    terminal::Terminal,
};

const LEAD: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Place {
    /// Nothing written yet: the next row starts with the symbol.
    Start,
    /// A row just ended: the next one starts with the rail.
    LineStart,
    /// A row is open.
    Inside,
}

#[derive(Debug)]
struct Flow {
    rail: String,
    symbol: String,
    width: Option<usize>,
    place: Place,
    row: String,
    held: String,
}

impl Flow {
    fn lead(&self) -> String {
        match self.place {
            Place::Start => self.symbol.clone(),
            Place::LineStart | Place::Inside => self.rail.clone(),
        }
    }

    fn feed(&mut self, chunk: &str) -> String {
        let last = chunk.split('\n').count().saturating_sub(1);
        chunk
            .split('\n')
            .enumerate()
            .map(|(at, part)| {
                let part = if at < last {
                    part.strip_suffix('\r').unwrap_or(part)
                } else {
                    part
                };
                let newline = if at == 0 {
                    String::new()
                } else {
                    self.newline()
                };
                self.held.push_str(part);
                newline + &self.settle(false)
            })
            .collect()
    }

    fn newline(&mut self) -> String {
        let flushed = self.settle(true);
        let open = match self.place {
            Place::Inside => String::new(),
            Place::Start | Place::LineStart => self.lead(),
        };
        self.place = Place::LineStart;
        self.row.clear();
        format!("{flushed}{open}\n")
    }

    /// Draws what is whole: all of `held` when `done`, else up to the last
    /// finished word, or all of it once its word is wider than a row. A wide
    /// character (width 2) ends a word too.
    fn settle(&mut self, done: bool) -> String {
        let held = std::mem::take(&mut self.held);
        let avail = self.width.map(|width| width.saturating_sub(LEAD).max(1));
        let whole = match avail {
            _ if done => held.trim_end_matches(' ').len(),
            None => held.len(),
            Some(avail) => {
                let spaced = held
                    .rsplit_once(' ')
                    .map_or(0, |(head, _)| head.trim_end_matches(' ').len());
                let wide = held
                    .grapheme_indices(true)
                    .rfind(|(_, grapheme)| measure_text_width(grapheme) >= 2)
                    .map_or(0, |(at, grapheme)| at + grapheme.len());
                let finished = spaced.max(wide);
                let word = held.get(finished..).unwrap_or_default();
                if measure_text_width(word.trim_start_matches(' ')) > avail {
                    held.len()
                } else {
                    finished
                }
            }
        };
        let (ready, rest) = held.split_at(whole);
        if !done {
            rest.clone_into(&mut self.held);
        }
        if ready.is_empty() {
            return String::new();
        }
        self.draw(ready, avail)
    }

    fn draw(&mut self, ready: &str, avail: Option<usize>) -> String {
        let opening = match self.place {
            Place::Inside => String::new(),
            Place::Start | Place::LineStart => format!("{}  ", self.lead()),
        };
        self.place = Place::Inside;
        let Some(avail) = avail else {
            return format!("{opening}{ready}");
        };
        let rows = wrap(&format!("{}{ready}", self.row), avail, "");
        let rail = format!("\n{}  ", self.rail);
        let mut rows = rows.iter();
        let first = rows.next().map_or("", String::as_str);
        let mut out = first.strip_prefix(self.row.as_str()).map_or_else(
            || format!("{opening}{rail}{first}"),
            |grown| format!("{opening}{grown}"),
        );
        let mut last = first;
        for row in rows {
            out.push_str(&rail);
            out.push_str(row);
            last = row;
        }
        last.clone_into(&mut self.row);
        out
    }

    fn close(&mut self) -> String {
        let flushed = self.settle(true);
        let end = match self.place {
            Place::Inside => "\n",
            Place::Start | Place::LineStart => "",
        };
        if self.place == Place::Inside {
            self.place = Place::LineStart;
        }
        self.row.clear();
        format!("{flushed}{end}")
    }

    fn finish(&mut self) -> String {
        let close = self.close();
        let empty = match self.place {
            Place::Start => format!("{}\n", self.lead()),
            Place::LineStart | Place::Inside => String::new(),
        };
        self.place = Place::LineStart;
        format!("{close}{empty}{}\n", self.rail)
    }
}

/// Text written in pieces; build it with [`Session::stream`]. Dropping it
/// before [`finish`](Self::finish) ends the open row and loses a write error;
/// with accessible output a row is written only when done.
///
/// ```no_run
/// # use clacky::{core::session::Session, render::draw::Tone, terminal::Terminal};
/// # fn show<T: Terminal>(session: &Session<T>) -> clacky::core::error::Result<()> {
/// let mut stream = session.stream(Tone::Info);
/// stream.write("Hello, wor")?;
/// stream.write("ld\nbye")?;
/// stream.finish()?;
/// # Ok(())
/// # }
/// ```
#[derive(Debug)]
pub struct LogStream<'s, T: Terminal> {
    session: &'s Session<T>,
    flow: Flow,
    pending: String,
}

impl<T: Terminal> Session<T> {
    /// Starts streamed text; nothing is written until the first chunk.
    pub fn stream(&self, tone: Tone) -> LogStream<'_, T> {
        let skin = self.skin();
        LogStream {
            session: self,
            flow: Flow {
                rail: skin.paint(skin.theme().rail, skin.symbols().bar),
                symbol: tone.symbol(skin),
                width: self.terminal().width(),
                place: Place::Start,
                row: String::new(),
                held: String::new(),
            },
            pending: String::new(),
        }
    }
}

impl<T: Terminal> LogStream<'_, T> {
    /// Draws a chunk; it may end in the middle of a row or of a word.
    /// # Errors
    /// The terminal failed.
    pub fn write(&mut self, chunk: &str) -> Result<()> {
        let text = self.flow.feed(chunk);
        let ready = self.ready(text);
        self.session.write(&ready)
    }

    fn ready(&mut self, text: String) -> String {
        match self.session.mode() {
            OutputMode::Animated => text,
            OutputMode::Accessible => {
                self.pending.push_str(&text);
                let done = self.pending.rfind('\n').map_or(0, |at| at + 1);
                self.pending.drain(..done).collect()
            }
        }
    }

    /// Ends the open row and leaves a rail row below, so the next element
    /// starts under the text.
    /// # Errors
    /// The terminal failed.
    pub fn finish(mut self) -> Result<()> {
        let text = self.flow.finish();
        let ready = self.ready(text);
        self.session.write(&ready)
    }

    /// Writes every chunk of `chunks`, then finishes.
    /// # Errors
    /// The terminal failed.
    pub fn pipe<S: AsRef<str>>(
        mut self,
        chunks: impl IntoIterator<Item = S>,
    ) -> Result<()> {
        chunks
            .into_iter()
            .try_for_each(|chunk| self.write(chunk.as_ref()))?;
        self.finish()
    }
}

impl<T: Terminal> Drop for LogStream<'_, T> {
    /// Ends the open row of an unfinished stream; a failed write is dropped.
    fn drop(&mut self) {
        let closed = self.flow.close();
        let text = self.ready(closed);
        if !text.is_empty() {
            self.session.write(&text).unwrap_or_default();
        }
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::{Flow, Place};
    use crate::{
        core::{
            session::Session,
            settings::{OutputMode, Settings},
        },
        fake::{Script, screen},
        render::{color::ColorChoice, draw::Tone},
        terminal::TerminalKind,
    };

    fn run(width: Option<usize>, chunks: &[&str]) -> String {
        let mut flow = Flow {
            rail: "│".to_owned(),
            symbol: "●".to_owned(),
            width,
            place: Place::Start,
            row: String::new(),
            held: String::new(),
        };
        let body: String = chunks.iter().map(|chunk| flow.feed(chunk)).collect();
        body + &flow.finish()
    }

    #[rstest]
    #[case(None, &["Hel", "lo"], "●  Hello\n│\n")]
    #[case(None, &["a\nb\n"], "●  a\n│  b\n│\n")]
    #[case(None, &["a\n", "\nb"], "●  a\n│\n│  b\n│\n")]
    #[case(None, &[], "●\n│\n")]
    #[case(Some(8), &["abcdefgh"], "●  abcde\n│  fgh\n│\n")]
    #[case(Some(8), &["ab世界c"], "●  ab世\n│  界c\n│\n")]
    #[case(Some(10), &["hello wor", "ld again"], "●  hello\n│  world\n│  again\n│\n")]
    #[case(Some(10), &["one", " two ", "three"], "●  one two\n│  three\n│\n")]
    #[case(Some(10), &["ab  cd"], "●  ab  cd\n│\n")]
    fn chunks_draw_on_the_rail(
        #[case] width: Option<usize>,
        #[case] chunks: &[&str],
        #[case] shown: &str,
    ) {
        assert_eq!(run(width, chunks), shown);
    }

    #[test]
    fn text_without_spaces_streams_after_each_wide_character() {
        let mut flow = Flow {
            rail: "│".to_owned(),
            symbol: "●".to_owned(),
            width: Some(80),
            place: Place::Start,
            row: String::new(),
            held: String::new(),
        };
        let shown: Vec<String> = "日本語のテキストです"
            .chars()
            .map(|c| flow.feed(&c.to_string()))
            .collect();
        assert!(shown.iter().all(|piece| !piece.is_empty()), "{shown:?}");
        assert_eq!(
            shown.concat() + &flow.finish(),
            "●  日本語のテキストです\n│\n"
        );
        assert_eq!(flow.feed("ab"), "");
    }

    #[test]
    fn session_stream_and_pipe_write() {
        let session = Session::new(
            Script::new(TerminalKind::Live, [], []),
            Settings::new(ColorChoice::Plain),
        );
        let done = session.stream(Tone::Success).pipe(["one\n", "tw", "o"]);
        assert!(done.is_ok());
        assert_eq!(screen(&session.terminal().written()), "◆  one\n│  two\n│");
    }

    #[test]
    fn accessible_output_writes_a_row_when_it_is_done() {
        let session = Session::new(
            Script::new(TerminalKind::Live, [], []),
            Settings::new(ColorChoice::Plain),
        )
        .output(OutputMode::Accessible);
        let mut stream = session.stream(Tone::Info);
        assert!(stream.write("hel").is_ok());
        assert!(stream.write("lo\nwor").is_ok());
        assert_eq!(session.terminal().written(), "●  hello\n");
        assert!(stream.write("ld").is_ok());
        assert!(stream.finish().is_ok());
        assert_eq!(session.terminal().written(), "●  hello\n│  world\n│\n");
    }

    #[test]
    fn an_unfinished_stream_closes_its_row_on_drop() {
        let session = Session::new(
            Script::new(TerminalKind::Live, [], []),
            Settings::new(ColorChoice::Plain),
        );
        let mut stream = session.stream(Tone::Info);
        assert!(stream.write("half").is_ok());
        drop(stream);
        assert_eq!(session.terminal().written(), "●  half\n");
    }
}
