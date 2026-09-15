# Enumerate ALL SMTC sessions + full metadata (multi-session check)
import asyncio, json
from winsdk.windows.media.control import GlobalSystemMediaTransportControlsSessionManager as Mgr

async def main():
    mgr = await Mgr.request_async()
    sessions = list(mgr.get_sessions())
    print("sessions:", len(sessions))
    for s in sessions:
        props = await s.try_get_media_properties_async()
        print("-", s.source_app_user_model_id, "|", repr(props.title), "|", repr(props.artist), "|", str(s.get_playback_info().playback_status))

asyncio.run(main())
