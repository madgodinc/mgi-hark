# madgodinc.net/hark/

The download page served from `tyan:/data/aurora-web/hark/`. Deploy the page and icon by hand:

```
scp site/index.html site/favicon.svg tyan:/data/aurora-web/hark/
```

Next to them on the server live the fonts (`onest-cyrillic.woff2`, `onest-latin.woff2`, `unbounded-latin.woff2`, copied from `node_modules/@fontsource-variable`), the screenshot `hark.png`, and the files `scripts/release.mjs` uploads: installers, `latest.json`, `history.json`. The page reads the current version from `latest.json`.
