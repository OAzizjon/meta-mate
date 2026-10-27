// Pure {placeholder} interpolation with a default {name} fallback.

export type TemplateVars = Readonly<Record<string, string | number | undefined>>;

const PLACEHOLDER_RE = /\{([^{}]+)\}/g;

/**
 * Replaces `{placeholder}` tokens in `template` using `vars`.
 *
 * `{name}` defaults to `defaultName` (the robot name passed in at init) unless
 * explicitly overridden in `vars`. Unknown placeholders are left untouched so
 * missing arguments remain visible in tests and diagnostics.
 */
export function interpolate(
  template: string,
  defaultName: string,
  vars?: TemplateVars,
): string {
  return template.replace(PLACEHOLDER_RE, (match, rawKey: string) => {
    if (vars !== undefined && Object.prototype.hasOwnProperty.call(vars, rawKey)) {
      const value = vars[rawKey];
      if (value !== undefined) {
        return String(value);
      }
    }
    if (rawKey === "name") {
      return defaultName;
    }
    return match;
  });
}
