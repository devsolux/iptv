use tauri::{AppHandle,Runtime,plugin::{Builder,TauriPlugin}};
#[cfg(target_os="android")]
use tauri::{Manager,plugin::PluginHandle};

#[derive(Clone,serde::Serialize)]
#[serde(rename_all="camelCase")]
pub struct PlaybackArgs {pub session_id:String,pub url:String,pub title:String,pub kind:String}
#[cfg(target_os="android")]
struct Player<R:Runtime>(PluginHandle<R>);

pub fn init<R:Runtime>()->TauriPlugin<R>{
    Builder::new("iptv-player").setup(|_app,_api|{
        #[cfg(target_os="android")]
        {
            let handle=_api.register_android_plugin("com.devsolux.iptv.player","PlayerPlugin")?;
            _app.manage(Player(handle));
        }
        Ok(())
    }).build()
}
/// Resolves when native playback ends, not when the dialog is merely opened.
pub fn play<R:Runtime>(_app:&AppHandle<R>,_args:PlaybackArgs)->Result<(),String>{
    #[cfg(target_os="android")]
    {
        let player=_app.state::<Player<R>>();
        let _:serde_json::Value=player.0.run_mobile_plugin("play",_args).map_err(|_|"Native player failed".to_owned())?;
        Ok(())
    }
    #[cfg(not(target_os="android"))]
    {Err("Native Android playback is not available on this platform".to_owned())}
}
pub fn stop<R:Runtime>(_app:&AppHandle<R>,_session_id:&str)->Result<(),String>{
    #[cfg(target_os="android")]
    {
        let player=_app.state::<Player<R>>();
        let _:serde_json::Value=player.0.run_mobile_plugin("stop",serde_json::json!({"sessionId":_session_id}))
            .map_err(|_|"Native player stop failed".to_owned())?;
    }
    Ok(())
}
