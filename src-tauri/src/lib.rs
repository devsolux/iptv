use iptv_core::{
    Core,
    error::{Error, PublicError},
    models::*,
};
use std::sync::{Arc, Mutex};
use tauri::{Emitter, Manager, State};
use tokio::sync::Semaphore;

struct AppState {
    core: Arc<Core>,
    queries: Arc<Semaphore>,
    playback: Mutex<Option<Playback>>,
}
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct AppStatus {
    name: &'static str,
    version: &'static str,
    identifier: &'static str,
    native_player: bool,
}
type Reply<T> = std::result::Result<T, PublicError>;

async fn worker<T: Send + 'static>(
    state: &AppState,
    operation: impl FnOnce(Arc<Core>) -> iptv_core::error::Result<T> + Send + 'static,
) -> Reply<T> {
    let core = state.core.clone();
    let _permit = state
        .queries
        .clone()
        .acquire_owned()
        .await
        .map_err(|_| Error::Cancelled.public())?;
    tokio::task::spawn_blocking(move || operation(core))
        .await
        .map_err(|_| Error::Storage.public())?
        .map_err(|e| e.public())
}
#[tauri::command]
fn app_status() -> AppStatus {
    AppStatus {
        name: "IPTV",
        version: "1.1.0",
        identifier: "com.devsolux.iptv",
        native_player: cfg!(target_os = "android"),
    }
}
#[tauri::command]
async fn list_sources(state: State<'_, AppState>) -> Reply<Vec<Source>> {
    worker(&state, |c| c.database.sources()).await
}
#[tauri::command]
async fn save_source(state: State<'_, AppState>, input: SourceInput) -> Reply<String> {
    worker(&state, move |c| c.database.save_source(&input, &c.vault)).await
}
#[tauri::command]
async fn remove_source(state: State<'_, AppState>, source_id: String) -> Reply<()> {
    worker(&state, move |c| c.remove_source(&source_id)).await
}
#[tauri::command]
async fn refresh_source(state: State<'_, AppState>, source_id: String) -> Reply<String> {
    worker(&state, move |c| c.start_job(source_id)).await
}
#[tauri::command]
fn job_status(state: State<'_, AppState>, job_id: String) -> Reply<Job> {
    state.core.job(&job_id).map_err(|e| e.public())
}
#[tauri::command]
fn cancel_job(state: State<'_, AppState>, job_id: String) -> Reply<()> {
    state.core.cancel(&job_id).map_err(|e| e.public())
}
#[tauri::command]
fn list_jobs(state: State<'_, AppState>) -> Reply<Vec<Job>> {
    state.core.jobs().map_err(|e| e.public())
}
#[tauri::command]
async fn query_countries(
    state: State<'_, AppState>,
    source_id: Option<String>,
) -> Reply<Vec<Country>> {
    worker(&state, move |c| c.database.countries(source_id.as_deref())).await
}
#[tauri::command]
async fn iptv_org_catalog(
    state: State<'_, AppState>,
    api_base: Option<String>,
    playlist_base: Option<String>,
) -> Reply<iptv_core::iptv_org::Catalog> {
    worker(&state, move |c| {
        c.iptv_org_catalog(api_base.as_deref(), playlist_base.as_deref())
    })
    .await
}
#[tauri::command]
async fn locate_channel(
    state: State<'_, AppState>,
    query: ChannelQuery,
    channel_id: String,
) -> Reply<ChannelLocation> {
    worker(&state, move |c| c.database.locate(&query, &channel_id)).await
}
#[tauri::command]
fn exit_app(app: tauri::AppHandle) {
    app.exit(0);
}
#[tauri::command]
async fn query_channels(state: State<'_, AppState>, query: ChannelQuery) -> Reply<ChannelPage> {
    worker(&state, move |c| c.channels(&query)).await
}
#[tauri::command]
async fn query_groups(state: State<'_, AppState>, source_id: Option<String>) -> Reply<Vec<String>> {
    worker(&state, move |c| c.database.groups(source_id.as_deref())).await
}
#[tauri::command]
async fn query_guide(state: State<'_, AppState>, query: GuideQuery) -> Reply<GuideWindow> {
    worker(&state, move |c| c.database.guide(&query)).await
}
#[tauri::command]
async fn set_favorite(state: State<'_, AppState>, channel_id: String, enabled: bool) -> Reply<()> {
    worker(&state, move |c| {
        c.database.favorite(&channel_id, enabled)?;
        c.notify_changed();
        Ok(())
    })
    .await
}
#[tauri::command]
async fn set_epg_mapping(
    state: State<'_, AppState>,
    channel_id: String,
    feed_id: Option<String>,
    epg_id: Option<String>,
) -> Reply<()> {
    worker(&state, move |c| {
        c.database
            .mapping(&channel_id, feed_id.as_deref(), epg_id.as_deref())?;
        c.notify_changed();
        Ok(())
    })
    .await
}
#[tauri::command]
async fn open_playback(
    state: State<'_, AppState>,
    channel_id: String,
    variant: u32,
) -> Reply<Playback> {
    let requested = channel_id.clone();
    let (_, title, secret, config, variants) = worker(&state, move |c| {
        c.database.stream(&requested, variant, &c.vault)
    })
    .await?;
    let prepared = state
        .core
        .relay
        .start(secret, &config)
        .await
        .map_err(|e| e.public())?;
    let playback = Playback {
        session_id: prepared.id,
        channel_id,
        title,
        url: prepared.url,
        kind: prepared.kind,
        native: cfg!(target_os = "android"),
        variants,
    };
    *state.playback.lock().map_err(|_| Error::Storage.public())? = Some(playback.clone());
    Ok(playback)
}
#[tauri::command]
async fn play_native(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    session_id: String,
) -> Reply<()> {
    let playback = state
        .playback
        .lock()
        .map_err(|_| Error::Storage.public())?
        .as_ref()
        .filter(|p| p.session_id == session_id)
        .cloned()
        .ok_or_else(|| Error::NotFound.public())?;
    let result = tokio::task::spawn_blocking(move || {
        tauri_plugin_iptv_player::play(
            &app,
            tauri_plugin_iptv_player::PlaybackArgs {
                session_id: playback.session_id,
                url: playback.url,
                title: playback.title,
                kind: playback.kind,
            },
        )
    })
    .await
    .map_err(|_| Error::Player.public())?;
    state.core.relay.close(Some(&session_id));
    if let Ok(mut active) = state.playback.lock()
        && active.as_ref().is_some_and(|p| p.session_id == session_id)
    {
        *active = None;
    }
    result.map_err(|_| Error::Player.public())
}
#[tauri::command]
async fn close_playback(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    session_id: String,
) -> Reply<()> {
    iptv_core::db::valid_id(&session_id).map_err(|e| e.public())?;
    state.core.relay.close(Some(&session_id));
    if let Ok(mut active) = state.playback.lock()
        && active.as_ref().is_some_and(|p| p.session_id == session_id)
    {
        *active = None;
    }
    native_stop(app, session_id).await
}
async fn native_stop(app: tauri::AppHandle, session_id: String) -> Reply<()> {
    tokio::task::spawn_blocking(move || tauri_plugin_iptv_player::stop(&app, &session_id))
        .await
        .map_err(|_| Error::Player.public())?
        .map_err(|_| Error::Player.public())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // The runtime is bounded for Lite-class devices; CPU-heavy work uses controlled workers.
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .max_blocking_threads(8)
        .enable_all()
        .build()
        .expect("Cannot initialize the IPTV runtime");
    tauri::async_runtime::set(runtime.handle().clone());
    let _runtime = runtime;
    tauri::Builder::default()
        .plugin(tauri_plugin_iptv_player::init())
        .setup(|app| {
            let directory = app.path().app_data_dir()?;
            let core = tauri::async_runtime::block_on(Core::open(directory))?;
            let mut changes = core.changes.subscribe();
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                while changes.changed().await.is_ok() {
                    let _ = handle.emit("catalogue-changed", ());
                }
            });
            app.manage(AppState {
                core,
                queries: Arc::new(Semaphore::new(3)),
                playback: Mutex::new(None),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            app_status,
            list_sources,
            save_source,
            remove_source,
            refresh_source,
            job_status,
            list_jobs,
            cancel_job,
            query_channels,
            query_groups,
            query_countries,
            iptv_org_catalog,
            locate_channel,
            exit_app,
            query_guide,
            set_favorite,
            set_epg_mapping,
            open_playback,
            play_native,
            close_playback
        ])
        .build(tauri::generate_context!())
        .expect("Unable to initialize IPTV")
        .run(|app, event| {
            if matches!(event, tauri::RunEvent::Exit)
                && let Some(state) = app.try_state::<AppState>()
            {
                state.core.shutdown();
            }
        });
}
