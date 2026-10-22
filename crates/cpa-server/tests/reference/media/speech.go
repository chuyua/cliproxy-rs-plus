package main

import "strings"

// SpeechConfig is the speech run's config (CLIProxyAPI v8.0.35): two xAI API keys on the
// capture server, a payload rule for the voice model, and passthrough headers, so the
// upstream Content-Type takes part in the audio type and error headers would show.
const SpeechConfig = `config-version: 8
server:
  host: 127.0.0.1
  port: PORT
management:
  disable-control-panel: true
requests:
  nonstream-keepalive-interval: 1
  passthrough-headers: true
  payload:
    override:
      - models:
          - name: grok-voice-tts-1.0
            protocol: openai
        params:
          language: de
access:
  api-keys: [client-key-1]
oauth:
  auth-dir: AUTHDIR
api-keys:
  xai:
    - name: xai-1
      base-url: http://UPSTREAM/v1
      keys:
        - api-key: sk-fake-xai-a
        - api-key: sk-fake-xai-b
`

func audio(contentType, body string) upstream {
	return upstream{Status: 200, Headers: [][2]string{{"Content-Type", contentType}}, Body: body}
}

// zstd frames (made with the zstd CLI): a speech body, a body whose decoded form is over
// 1 MiB, and a body compressed twice ("zstd, zstd").
const (
	zstdHello    = "\x28\xb5\x2f\xfd\x00\x68\x21\x01\x00\x7b\x22\x69\x6e\x70\x75\x74\x22\x3a\x22\x7a\x73\x74\x64\x20\x68\x65\x6c\x6c\x6f\x22\x2c\x22\x76\x6f\x69\x63\x65\x22\x3a\x22\x61\x73\x68\x22\x7d"
	zstdTooLarge = "\x28\xb5\x2f\xfd\x00\x68\x9c\x00\x00\x58\x7b\x22\x69\x6e\x70\x75\x74\x22\x3a\x22\x61\x01\x00\xf2\xff\x39\xf0\x02\x02\x00\x10\x61\x02\x00\x10\x61\x02\x00\x10\x61\x02\x00\x10\x61\x02\x00\x10\x61\x02\x00\x10\x61\x02\x00\x10\x61\x45\x00\x00\x10\x22\x7d\x01\x00\x1b\x00\x02"
	zstdStacked  = "\x28\xb5\x2f\xfd\x00\x68\x99\x01\x00\x28\xb5\x2f\xfd\x00\x68\x51\x01\x00\x7b\x22\x74\x65\x78\x74\x22\x3a\x22\x73\x74\x61\x63\x6b\x65\x64\x22\x2c\x22\x72\x65\x73\x70\x6f\x6e\x73\x65\x5f\x66\x6f\x72\x6d\x61\x74\x22\x3a\x22\x77\x61\x76\x22\x7d"
)

func zstdPost(name, path, body string, encoding string, replies ...upstream) scenario {
	s := post(name, path, jsonType, body, replies...)
	s.Headers = map[string]string{"Content-Encoding": encoding}
	return s
}

func withSkipPrefix(s scenario, n int) scenario {
	s.SkipPrefix = n
	return s
}

func speechScenarios() []scenario {
	const speech = "/v1/audio/speech"
	const tts = "/v1/tts"
	return []scenario{
		post("speech_defaults", speech, jsonType,
			`{"input":"  Hello <world> & you  ","voice":" Nova "}`,
			upstream{Status: 200, Headers: [][2]string{{"Content-Type", "audio/mpeg"}, {"Cache-Control", "no-store"}}, Body: "ID3-mp3-bytes"}),
		post("speech_wav_speed", speech, jsonType,
			`{"model":"TTS-1-HD","input":"wave","voice":"alloy","language":" en ","response_format":" WAV ","speed":1.25}`,
			audio("application/octet-stream", "RIFF-wav-bytes")),
		post("tts_native_pcm", tts, jsonType,
			`{"model":"xai/grok-voice-tts-1.0(high)","text":"native","voice_id":" Rex ","output_format":{"codec":"PCM","sample_rate":16000.9},"speed":0}`,
			audio("audio/pcm;rate=16000", "pcm-bytes")),
		post("tts_native_zero_rate", tts, jsonType,
			`{"model":"grok/grok-tts","input":"x","voice":"unknown-Voice","output_format":{"codec":"wav","sample_rate":0},"speed":"2"}`,
			audio("text/plain; charset=utf-8", "wav-bytes")),
		post("speech_response_format_wins", speech, jsonType,
			`{"model":"gpt-4o-mini-tts","input":"x","response_format":"mp3","output_format":{"codec":"wav"},"speed":1e400}`),
		post("speech_mp3_native", speech, jsonType,
			`{"input":"x","output_format":{"codec":"MP3","sample_rate":44100},"speed":3}`,
			audio("", "mp3-bytes")),
		post("speech_unknown_model", speech, jsonType,
			`{"model":"gpt-4o","input":"x"}`),
		post("speech_unknown_model_prefixed", speech, jsonType,
			`{"model":" xai/grok-4 ","input":"x"}`),
		// Each invalid UTF-8 byte is one rune and one \ufffd.
		post("speech_invalid_utf8", speech, jsonType,
			"{\"input\":\"a\xe2\x82b\",\"voice\":\" X\xffY \"}",
			audio("audio/mpeg", "utf8")),
		post("speech_missing_input", speech, jsonType,
			`{"input":"   ","text":""}`),
		post("speech_invalid_json", speech, jsonType, `{"input":"x"`),
		post("speech_bad_response_format", speech, jsonType,
			`{"input":"x","response_format":"flac","speed":1e400}`),
		post("speech_bad_codec", tts, jsonType,
			`{"input":"x","output_format":{"codec":"opus"}}`),
		post("speech_output_format_not_object", tts, jsonType,
			`{"input":"x","output_format":"wav"}`),
		post("speech_too_long", speech, jsonType,
			`{"input":"`+strings.Repeat("a", 60001)+`"}`),
		// 30001 runes in 60002 bytes: the limit counts runes.
		post("speech_multibyte_under_limit", speech, jsonType,
			`{"input":"`+strings.Repeat("é", 30001)+`"}`,
			audio("audio/mpeg", "long")),
		post("speech_upstream_400", speech, jsonType,
			`{"input":"x","language":"zz"}`,
			status(400, `{"error":"invalid language"}`)),
		// Speech errors carry no upstream headers, even with passthrough on.
		post("speech_upstream_400_headers", speech, jsonType,
			`{"input":"x","language":"yy"}`,
			upstream{Status: 400, Headers: [][2]string{{"Content-Type", "text/plain"}, {"Cache-Control", "max-age=60"}, {"Retry-After", "7"}}, Body: "bad language"}),
		// Content-Encoding is decoded before the size check and the JSON parse.
		zstdPost("speech_zstd", speech, zstdHello, "zstd",
			audio("audio/mpeg", "zstd-audio")),
		zstdPost("tts_zstd_stacked", tts, zstdStacked, "zstd, zstd",
			audio("audio/wav", "stacked-audio")),
		zstdPost("speech_zstd_decoded_too_large", speech, zstdTooLarge, "zstd"),
		// 3 MiB on the wire, a small body once decoded: the size check is on the decoded body.
		withSkipPrefix(zstdPost("speech_zstd_skippable_3mib", speech, zstdHello, "zstd",
			audio("audio/mpeg", "skipped-audio")), 3<<20),
		post("speech_unknown_voice_404", speech, jsonType,
			`{"input":"x","voice":"nope"}`,
			status(404, `{"error":"voice not found"}`),
			status(404, `{"error":"voice not found"}`)),
		post("speech_after_unknown_voice", speech, jsonType,
			`{"input":"again"}`,
			audio("audio/mpeg", "ok")),
		post("speech_slow", speech, jsonType,
			`{"input":"slow"}`,
			slow(audio("audio/mpeg", "slow-audio"))),

		// --- speech-only models elsewhere ---
		post("chat_rejects_tts", "/v1/chat/completions", jsonType,
			`{"model":"grok-tts","messages":[{"role":"user","content":"hi"}]}`),
		post("chat_stream_rejects_tts", "/v1/chat/completions", jsonType,
			`{"model":"grok-tts","stream":true,"messages":[{"role":"user","content":"hi"}]}`),
		post("responses_rejects_voice_tts", "/v1/responses", jsonType,
			`{"model":"xai/Grok-Voice-TTS-1.0","input":"hi"}`),
		post("messages_rejects_tts", "/v1/messages", jsonType,
			`{"model":"grok-tts(high)","max_tokens":8,"messages":[{"role":"user","content":"hi"}]}`),

		// --- GET /v1/models for Grok Shell (no upstream call) ---
		{Name: "models_grok_shell", Method: "GET", Path: "/v1/models",
			Headers: map[string]string{"User-Agent": "grok-pager/0.2.119 grok-shell/0.2.119 (macos; aarch64)"}},

		// --- last: a model-unavailable 404 rotates and cools grok-tts on both keys ---
		post("speech_model_not_found_404", speech, jsonType,
			`{"input":"x"}`,
			status(404, `{"error":{"code":"model_not_found","message":"The model grok-tts does not exist"}}`),
			status(404, `{"error":{"code":"model_not_found","message":"The model grok-tts does not exist"}}`)),
	}
}
