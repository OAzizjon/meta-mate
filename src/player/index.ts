// The timeline player's public face: `createTimelinePlayer` wires `<lang>` to the app's locale
// (src/i18n). Node tests import ./timeline-player.ts directly, since they can't load the locale
// JSON that src/i18n imports.
import { setLocale } from "../i18n/index.ts";
import { TimelinePlayer } from "./timeline-player.ts";
import type { TimelinePlayerOptions } from "./timeline-player.ts";

export { TimelinePlayer };
export type { PlayerAudio, PlayerRobot, TimelinePlayerOptions } from "./timeline-player.ts";

export type CreateTimelinePlayerOptions = Omit<TimelinePlayerOptions, "setLocale"> &
  Partial<Pick<TimelinePlayerOptions, "setLocale">>;

export function createTimelinePlayer(options: CreateTimelinePlayerOptions): TimelinePlayer {
  return new TimelinePlayer({ ...options, setLocale: options.setLocale ?? setLocale });
}
