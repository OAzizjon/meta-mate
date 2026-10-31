import type { Plugin } from "vite";

/** Serves /sounds/<kind>/<name>.ogg from assets/sounds in dev and copies them into dist/sounds. */
export function sounds(): Plugin;
