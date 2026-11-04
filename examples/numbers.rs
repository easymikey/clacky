//! Numbers and typed text: ranges, steps, a check of your own and `parse`.

use std::net::Ipv4Addr;

use clacky::prelude::*;

fn main() -> Result<()> {
    let ui = Session::detect()?;
    ui.intro(" numbers ")?;

    let port: u16 = ui.ask(number("Port").min(1024).max(65535).default(8080))?;
    let ratio = ui.ask(
        number::<f64>("Ratio")
            .min(0.0)
            .max(1.0)
            .step(0.1)
            .default(0.5),
    )?;
    let workers = ui.ask(number::<u32>("Workers").validate(|count: &u32| {
        if count % 2 == 0 {
            Ok(())
        } else {
            Err("Use an even number.")
        }
    }))?;
    let address =
        ui.ask(text("Address").placeholder("127.0.0.1").parse::<Ipv4Addr>())?;

    ui.outro(format_args!(
        "{address}:{port}, {workers} workers at {ratio}"
    ))
}
