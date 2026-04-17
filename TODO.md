- change how stack allocations work when fetching data, may wait until we get our device, ex. cal_xml
- unfold lines before pasing in vcalendar response
- make display asynchronous

fix crash:
```
[INFO ] Making calendar request for date: 2026-04-17 (esp32_thesis esp32-thesis/src/networking.rs:135)
[DEBUG] Response status: StatusCode(207) (esp32_thesis esp32-thesis/src/networking.rs:235)
[WARN ] Incomplete chunked vcal data, waiting for more data to arrive (esp32_thesis esp32-thesis/src/process.rs:249)
[INFO ] Finished parsing calendar events, total events parsed: 3 (esp32_thesis esp32-thesis/src/process.rs:276)
[INFO ] Parsed calendar data: "[VEventData { summary: Some(\"Szieszta\"), dtstart: Some(2026-04-17T10:30:00Z), dtend: Some(2026-04-17T11:30:00Z) }, VEventData { summary: Some(\"Éjfél\"), dtstart: Some(2026-04-17T22:00:00Z), dtend: Some(2026-04-17T23:00:00Z) }, VEventData { summary: Some(\"Este\"), dtstart: Some(2026-04-17T21:00:00Z), dtend: Some(2026-04-17T21:59:00Z) }]" (esp32_thesis esp32-thesis/src/networking.rs:239)
[WARN ] Session dropped without being closed properly (mbedtls_rs src/session/asynch.rs:236)
[DEBUG] Response status: StatusCode(207) (esp32_thesis esp32-thesis/src/networking.rs:235)
[WARN ] Incomplete chunked vcal data, waiting for more data to arrive (esp32_thesis esp32-thesis/src/process.rs:249)
[INFO ] Finished parsing calendar events, total events parsed: 2 (esp32_thesis esp32-thesis/src/process.rs:276)
[INFO ] Parsed calendar data: "[VEventData { summary: Some(\"Reggeli\"), dtstart: Some(2026-04-17T06:00:00Z), dtend: Some(2026-04-17T07:00:00Z) }, VEventData { summary: Some(\"Biliárd\"), dtstart: Some(2026-04-17T18:30:00Z), dtend: Some(2026-04-17T21:00:00Z) }]" (esp32_thesis esp32-thesis/src/networking.rs:239)
[WARN ] Session dropped without being closed properly (mbedtls_rs src/session/asynch.rs:236)
[WARN ] Event 'Éjfél' is out of display bounds (2026-04-18T00:00:00+02:00[+02:00]-2026-04-18T01:00:00+02:00[+02:00]), skipping (esp32_thesis esp32-thesis/src/display.rs:249)
[WARN ] Event 'Este' is out of display bounds (2026-04-17T23:00:00+02:00[+02:00]-2026-04-17T23:59:00+02:00[+02:00]), skipping (esp32_thesis esp32-thesis/src/display.rs:249)
[WARN ] Event 'Reggeli' is out of display bounds (2026-04-17T08:00:00+02:00[+02:00]-2026-04-17T09:00:00+02:00[+02:00]), skipping (esp32_thesis esp32-thesis/src/display.rs:249)


====================== PANIC ======================
panicked at src/display.rs:272:28:
attempt to subtract with overflow
```
