# Quickstart: checking the redesign

## Gate

```bash
TMPDIR=<scratch dir> make ci
```

Includes `assets_total_under_150kb` and the new `theme_contrast_meets_aa` test.

## Changing accents

```bash
python3 specs/012-ui-themes/themes.py
```

Prints the `[data-accent]` rules to paste into `app.css`; exits 1 and lists the pairs that miss
WCAG AA.

## In a browser

```bash
cargo run -p putio-sync-api --example mock_ui 15572
```

Open `http://127.0.0.1:15572/`, sign in with the token the example prints, then:

1. Every page at 1280 px and 390 px, light and dark: sidebar or tab bar, no sideways scroll,
   last row visible above the tab bar.
2. Click the mode button in the nav: System → Light → Dark; reload: the choice stays, no flash.
3. Settings → Appearance: pick Teal; every page and the Stats charts turn teal.
4. Tab through a page: focus ring visible on every control, order is nav then content.
5. With reduced motion on (DevTools rendering panel), route changes and toasts do not animate.
6. Empty states: the mock's empty queue / active downloads show the empty card.
