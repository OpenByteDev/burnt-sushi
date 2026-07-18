use log::{debug, error};
use winrt_toast::{Action, Text, Toast, ToastManager};

const POWERSHELL_APP_ID: &str =
    "{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}\\WindowsPowerShell\\v1.0\\powershell.exe";

/// Shows a toast with the given title/body and one action button per `(label, id)` pair.
/// Resolves to the `id` of the action the user clicked, or `None` if dismissed/ignored/failed.
pub async fn show<Id: Clone + Send + 'static>(
    title: &str,
    body: &str,
    actions: &[(&str, Id)],
) -> Option<Id> {
    let manager = ToastManager::new(POWERSHELL_APP_ID);

    let mut toast = Toast::new();
    toast.text1(title).text2(Text::new(body));
    for (i, (label, _)) in actions.iter().enumerate() {
        toast.action(Action::new(*label, i.to_string(), i.to_string()));
    }

    let (tx, mut rx) = tokio::sync::mpsc::channel::<Option<usize>>(1);
    let (tx_activated, tx_dismissed, tx_failed) = (tx.clone(), tx.clone(), tx);

    let result = manager.show_with_callbacks(
        &toast,
        Some(Box::new(move |res| {
            let clicked = match res {
                Ok(id) => {
                    debug!("Toast activated (id={id})");
                    id.parse().ok()
                }
                Err(err) => {
                    debug!("Toast activation failed (err={err})");
                    None
                }
            };
            let _ = tx_activated.try_send(clicked);
        })),
        Some(Box::new(move |res| {
            match res {
                Ok(reason) => debug!("Toast dismissed (reason={reason:?})"),
                Err(err) => debug!("Toast dismissal failed (err={err})"),
            }
            let _ = tx_dismissed.try_send(None);
        })),
        Some(Box::new(move |err| {
            error!("Toast failed: {err}");
            let _ = tx_failed.try_send(None);
        })),
    );

    if let Err(err) = result {
        error!("Failed to show toast: {err}");
        return None;
    }

    let index = rx.recv().await.flatten()?;
    actions.get(index).map(|(_, id)| id.clone())
}