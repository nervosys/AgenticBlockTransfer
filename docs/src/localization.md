# Localization

`abt` supports 12 locales with runtime language detection.

## Supported Locales

| Code | Language   | Built-in Catalog |
| ---- | ---------- | ---------------- |
| en   | English    | ✅ (~35 messages) |
| de   | German     | ✅ (~35 messages) |
| fr   | French     | ✅ (~35 messages) |
| es   | Spanish    | ✅ (~35 messages) |
| pt   | Portuguese | —                |
| ja   | Japanese   | —                |
| zh   | Chinese    | —                |
| ko   | Korean     | —                |
| ru   | Russian    | —                |
| ar   | Arabic     | —                |
| it   | Italian    | —                |
| nl   | Dutch      | —                |

## Locale Detection

`abt` auto-detects the system locale from environment variables:

1. `LC_ALL`
2. `LC_MESSAGES`
3. `LANG`

Falls back to English if no match or variable is unset.

## Custom Translations

Load additional translations from a JSON file:

```json
{
  "app.name": "AgenticBlockTransfer",
  "write.started": "Schreibvorgang gestartet",
  "write.complete": "Schreibvorgang abgeschlossen",
  "device.confirm": "Auf Gerät {0} schreiben? [j/N]"
}
```

## Message Format

Messages support positional arguments:

```
"write.progress" → "Writing {0} of {1} ({2}/s)"
"device.confirm" → "Write to {0}? [y/N]"
```

## Message Categories

| Category | Prefix     | Examples                          |
| -------- | ---------- | --------------------------------- |
| App      | `app.*`    | app.name, app.version             |
| Write    | `write.*`  | write.started, write.complete     |
| Device   | `device.*` | device.selected, device.confirm   |
| Error    | `error.*`  | error.not_found, error.permission |
| Safety   | `safety.*` | safety.blocked, safety.warning    |
| UI       | `ui.*`     | ui.browse, ui.cancel              |
