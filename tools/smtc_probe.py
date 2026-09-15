# Machine-verified SMTC probe: reads the REAL Windows now-playing state via WinRT.
# Proof that Halftone's SMTC registration works: title/artist/album/status/timeline.
import asyncio, sys

async def main():
    from winsdk.windows.media.control import (
        GlobalSystemMediaTransportControlsSessionManager as Mgr,
    )
    mgr = await Mgr.request_async()
    sessions = list(mgr.get_sessions())
    if not sessions:
        print("NO_SESSIONS")
        return
    out = []
    for s in sessions:
        props = await s.try_get_media_properties_async()
        tp = s.get_timeline_properties()
        out.append({
            "src": s.source_app_user_model_id,
            "title": props.title,
            "artist": props.artist,
            "album": props.album_title,
            "status": str(s.get_playback_info().playback_status),
            "pos": round(tp.position.total_seconds(), 1) if tp.position else 0,
            "end": round(tp.end_time.total_seconds(), 1) if tp.end_time else 0,
        })
    print(__import__("json").dumps(out, indent=1))

asyncio.run(main())
