//! The clack demo flow: intro, a form, a spinner, a note and an outro.

use std::{thread::sleep, time::Duration};

use clacky::{NoteLine, NoteTone, output::spinner::DELAY, prelude::*};

fn main() -> Result<()> {
    let ui = Session::detect()?;
    ui.intro(" create-app ")?;

    let asked = ui
        .ask((
            text("Where should we create your project?")
                .placeholder("./sparkling-solid")
                .validate(|path: &str| {
                    if path.is_empty() {
                        Err("Please enter a path.")
                    } else {
                        Ok(())
                    }
                }),
            select("Pick a project type")
                .item("ts", "TypeScript")
                .item("js", "JavaScript")
                .item("coffee", "CoffeeScript")
                .hint("oh no"),
            multiselect("Select additional tools")
                .item("eslint", "ESLint")
                .item("prettier", "Prettier")
                .item("gh-action", "GitHub Action")
                .picked([0, 1])
                .optional(),
            confirm("Install dependencies?"),
        ))
        .cancel_as_none()?;
    let Some((path, kind, tools, install)) = asked else {
        return ui.cancel("Operation cancelled");
    };

    if install {
        let mut spinner = ui.spinner();
        spinner.start("Installing via pnpm")?;
        for _ in 0..20 {
            sleep(DELAY);
            spinner.tick()?;
        }
        spinner.stop("Installed via pnpm")?;
    }

    let next_steps = [
        format!("cd {path}"),
        if install {
            String::new()
        } else {
            "pnpm install".to_owned()
        },
        "pnpm dev".to_owned(),
    ];
    let lines: Vec<NoteLine> = next_steps
        .into_iter()
        .filter(|step| !step.is_empty())
        .map(|text| NoteLine {
            tone: NoteTone::Plain,
            text,
        })
        .collect();
    ui.note("Next steps.", &lines)?;
    ui.log(
        clacky::Tone::Info,
        format_args!("{kind} with {} tools", tools.len()),
    )?;
    sleep(Duration::from_millis(200));
    ui.outro("Problems? https://example.com/issues")
}
