# Controller contract lab

This SvelteKit application is the phone-friendly view of the throwaway
controller conformance protocol prototype. It models the same startup,
authority, command, timeout, rejection, fault, and recovery semantics as the
terminal prototype without talking to vehicle hardware.

From this directory:

```sh
npm install
npm run dev
```

The application is prerendered with `@sveltejs/adapter-static`. GitHub Actions
sets `BASE_PATH=/celerity` and deploys the resulting `build/` directory to
GitHub Pages.
