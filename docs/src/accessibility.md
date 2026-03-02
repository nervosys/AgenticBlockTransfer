# Accessibility

`abt` includes comprehensive accessibility support for TUI and GUI modes.

## Features

| Feature        | Description                                      |
| -------------- | ------------------------------------------------ |
| Screen reader  | ARIA-like role announcements for all UI elements |
| High contrast  | WCAG 2.1 AA compliant color palette              |
| Keyboard-only  | Full navigation without mouse                    |
| Reduced motion | Disables animations and transitions              |
| Large text     | Enlarged font sizes for readability              |

## ARIA Roles

16 semantic roles for screen reader integration:

`Button`, `TextInput`, `List`, `ListItem`, `ProgressBar`, `Label`, `Heading`, `Alert`, `Dialog`, `Status`, `Navigation`, `Menu`, `MenuItem`, `Checkbox`, `TabPanel`, `Toolbar`

## High-Contrast Palette

All color combinations meet WCAG 2.1 AA minimum contrast ratio (4.5:1 for text):

| Element    | Color   | Contrast Ratio |
| ---------- | ------- | -------------- |
| Background | #000000 | —              |
| Foreground | #FFFFFF | 21:1           |
| Accent     | #00BFFF | 8.6:1          |
| Success    | #00FF00 | 15.3:1         |
| Error      | #FF4444 | 5.3:1          |
| Warning    | #FFD700 | 14.0:1         |

## Announcement Queue

UI state changes are queued for screen reader announcement with priority levels:

| Priority | Example                   |
| -------- | ------------------------- |
| Low      | Theme changed             |
| Normal   | Device selected           |
| High     | Write started             |
| Critical | Write failed, error alert |

## Environment Variables

| Variable         | Description                  |
| ---------------- | ---------------------------- |
| `SCREEN_READER`  | Enable screen reader mode    |
| `HIGH_CONTRAST`  | Enable high contrast palette |
| `REDUCED_MOTION` | Disable animations           |
| `LARGE_TEXT`     | Enable large text mode       |
