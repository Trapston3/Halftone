//! Windows System Media Transport Controls (SMTC) integration.
//! Only the audio OWNER drives this (main window commands only).
//! Creates the OS now-playing overlay + global media keys.
#![cfg(target_os = "windows")]

use windows::Foundation::TypedEventHandler;
use windows::Media::{
    MediaPlaybackStatus, MediaPlaybackType, SystemMediaTransportControls,
    SystemMediaTransportControlsButton, SystemMediaTransportControlsButtonPressedEventArgs,
};
use windows::core::HSTRING;

pub struct Smtc {
    smtc: SystemMediaTransportControls,
}

impl Smtc {
    pub fn new<F>(on_button: F) -> Option<Self>
    where
        F: Fn(&str) + Send + Sync + 'static,
    {
        let smtc = SystemMediaTransportControls::GetForCurrentView().ok()?;  // WinRT static factory
        smtc
            .ButtonPressed(&TypedEventHandler::<
                SystemMediaTransportControls,
                SystemMediaTransportControlsButtonPressedEventArgs,
            >::new(move |_sender, args| {
                if let Some(args) = args {
                    if let Ok(btn) = args.Button() {
                        let ev = match btn {
                            SystemMediaTransportControlsButton::Play => Some("play"),
                            SystemMediaTransportControlsButton::Pause => Some("pause"),
                            SystemMediaTransportControlsButton::Next => Some("next"),
                            SystemMediaTransportControlsButton::Previous => Some("prev"),
                            SystemMediaTransportControlsButton::Stop => Some("pause"),
                            _ => None,
                        };
                        if let Some(ev) = ev {
                            on_button(ev);
                        }
                    }
                }
                Ok(())
            }))
            .ok()?;  /* handler passed by ref: Param<T> for &T (CloneType) */
        Some(Self { smtc })
    }

    /// Populate the OS now-playing overlay. Cover art comes from the
    /// METADATA_BLOCK_PICTURE bytes already in memory (base64) — never a
    /// re-read of the file, never a network fetch.
    pub fn set_metadata(&self, title: &str, artist: &str, album: &str, cover_b64: Option<&str>) {
        let Ok(updater) = self.smtc.DisplayUpdater() else { return };
        let _ = updater.SetType(MediaPlaybackType::Music);
        if let Ok(music) = updater.MusicProperties() {
            let _ = music.SetTitle(&HSTRING::from(title));
            let _ = music.SetArtist(&HSTRING::from(artist));
            let _ = music.SetAlbumTitle(&HSTRING::from(album));
        }
        if let Some(b64) = cover_b64 {
            if let Some(bytes) = crate::base64_decode_pub(b64) {
                use windows::Storage::Streams::{DataWriter, InMemoryRandomAccessStream};
                if let Ok(stream) = InMemoryRandomAccessStream::new() {
                    if let Ok(writer) = DataWriter::CreateDataWriter(&stream) {
                        let _ = writer.WriteBytes(&bytes);
                        let _ = writer.StoreAsync();
                        if let Ok(refstr) = windows::Storage::Streams::RandomAccessStreamReference::CreateFromStream(&stream) {
                            let _ = updater.SetThumbnail(&refstr);
                        }
                    }
                }
            }
        }
        let _ = updater.Update();
    }

    pub fn set_status(&self, playing: bool) {
        let st = if playing {
            MediaPlaybackStatus::Playing
        } else {
            MediaPlaybackStatus::Paused
        };
        let _ = self.smtc.SetPlaybackStatus(st);
    }

    pub fn set_enabled(&self, next: bool, prev: bool, pause: bool, play: bool) {
        let _ = self.smtc.SetIsNextEnabled(next);
        let _ = self.smtc.SetIsPreviousEnabled(prev);
        let _ = self.smtc.SetIsPauseEnabled(pause);
        let _ = self.smtc.SetIsPlayEnabled(play);
    }
}
