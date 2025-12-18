# ==========================================
# RFM Liquidsoap Encoder – 2025 Version
# ==========================================

log.file.path := "/home/rfm/airlift_liq.log"
log.file := true
log.stdout := false
log.level := 3

radio =
  mksafe(
    input.external.rawaudio(
      samplerate=48000,
      channels=2,
      buffer=0.5,
      max=10.,
      restart=true,
      restart_on_error=true,
      "sh -c 'cat <&0'"
    )
  )

# Lokaler Icecast (192.168.0.20) – OGG
output.icecast(
  %vorbis,
  host="192.168.0.20",
  port=8000,
  password="rfm_4dmin",
  mount="rfm.ogg",
  url="http://icecast.radiorfm.de",
  name="RFM OGG Local",
  description="RFM OGG (local)",
  radio
)

# Lokaler Icecast (192.168.0.20) – MP3
output.icecast(
  %mp3(bitrate=192),
  host="192.168.0.20",
  port=8000,
  password="rfm_4dmin",
  mount="rfm.mp3",
  url="http://icecast.radiorfm.de",
  name="RFM MP3 Local",
  description="RFM MP3 (local)",
  radio
)
