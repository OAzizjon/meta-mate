# MetaMate landing page

Static landing page for the adult child who installs Mate for a parent:

- [`index.html`](index.html) — page markup (hero, "How it works" in 3 steps, "Keepsake lesson cards", scam check "Is this safe?", "Safety and privacy", "Built with Meta", and `#download` call to action).
- [`style.css`](style.css) — responsive, accessible stylesheet using system fonts and light/dark high-contrast palettes.

The hero image references [`../assets/art/mate.png`](../assets/art/mate.png) by relative path rather than duplicating binary assets.

## How to preview

### 1. Open directly in a browser

Open `site/index.html` in Edge, Chrome, or Firefox. Because the page uses no JavaScript modules or external requests, `file://` preview works directly and resolves `../assets/art/mate.png`.

### 2. Serve locally over HTTP

Run a static server from the **repository root** (not inside `site/`, so that `/assets/art/mate.png` stays within the served root):

```powershell
python -m http.server 8000 --bind 127.0.0.1
```

Then open `http://127.0.0.1:8000/site/` in your browser. Use browser DevTools device toolbar (for example, `375px` width) to inspect the mobile layout.
