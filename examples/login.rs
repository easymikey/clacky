//! A login: text, then a password that is asked again while the attempt fails.

use std::num::NonZeroU8;

use clacky::prelude::*;

fn sign_in(user: &str, secret: &str) -> std::result::Result<(), String> {
    if secret == "hunter2" {
        Ok(())
    } else {
        Err(format!("wrong password for {user}"))
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let ui = Session::detect()?;
    ui.intro("login")?;

    let user = ui.ask(text("User name").default("ada"))?;

    let tries = NonZeroU8::MIN.saturating_add(2);
    let outcome = ui
        .ask_until(
            || password("Password").mask('*'),
            "Signing in",
            async |secret: &zeroize::Zeroizing<String>| sign_in(&user, secret),
            |refused: &String| Some(refused.clone()),
            tries,
        )
        .await?;
    match outcome {
        Ok(()) => ui.outro("Welcome back"),
        Err(refused) => ui.cancel_with("Could not sign in", [refused]),
    }
}
