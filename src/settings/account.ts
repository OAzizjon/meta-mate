// Pure account state machine, validation, and store (WP67b).
// No DOM or direct Tauri imports so Node (`node --test`) runs this module directly.

export const AUTH_PROVIDERS = ["google", "github"] as const;
export type AuthProvider = (typeof AUTH_PROVIDERS)[number];

export const AUTH_CHANGED_EVENT = "auth-changed" as const;

export interface AuthStatus {
  readonly signedIn: boolean;
  readonly name?: string;
  readonly provider?: AuthProvider;
}

export interface SignInResult {
  readonly name: string;
  readonly provider: AuthProvider;
}

export type AccountStatus = "signed_out" | "signing_in" | "signed_in";

export interface SignedOutState {
  readonly status: "signed_out";
  readonly error?: string | null;
}

export interface SigningInState {
  readonly status: "signing_in";
  readonly provider: AuthProvider;
}

export interface SignedInState {
  readonly status: "signed_in";
  readonly name: string;
  readonly provider: AuthProvider;
}

export type AccountState = SignedOutState | SigningInState | SignedInState;

export type AccountAction =
  | { readonly type: "SIGN_IN_START"; readonly provider: AuthProvider }
  | { readonly type: "SIGN_IN_SUCCESS"; readonly name: string; readonly provider: AuthProvider }
  | { readonly type: "SIGN_IN_ERROR"; readonly error: string }
  | { readonly type: "SIGN_OUT_SUCCESS" }
  | { readonly type: "AUTH_CHANGED"; readonly payload: AuthStatus }
  | { readonly type: "RESET_ERROR" };

export const INITIAL_ACCOUNT_STATE: AccountState = Object.freeze({
  status: "signed_out",
  error: null,
});

export function isAuthProvider(value: unknown): value is AuthProvider {
  return value === "google" || value === "github";
}

export function isAuthStatus(raw: unknown): raw is AuthStatus {
  if (typeof raw !== "object" || raw === null) {
    return false;
  }
  const obj = raw as Record<string, unknown>;
  if (typeof obj.signedIn !== "boolean") {
    return false;
  }
  if (obj.name !== undefined && typeof obj.name !== "string") {
    return false;
  }
  if (obj.provider !== undefined && !isAuthProvider(obj.provider)) {
    return false;
  }
  return true;
}

/** The locale keys `sign_in` rejects with (src-tauri/src/auth.rs). */
const AUTH_ERRORS: readonly string[] = ["auth.failed", "auth.cancelled", "auth.unavailable", "auth.busy"];

/** Pure transition function for the account state machine. */
export function accountReducer(state: AccountState, action: AccountAction): AccountState {
  switch (action.type) {
    case "SIGN_IN_START":
      return {
        status: "signing_in",
        provider: action.provider,
      };

    case "SIGN_IN_SUCCESS":
      return {
        status: "signed_in",
        name: action.name,
        provider: action.provider,
      };

    case "SIGN_IN_ERROR":
      return {
        status: "signed_out",
        error: action.error,
      };

    case "SIGN_OUT_SUCCESS":
      return {
        status: "signed_out",
        error: null,
      };

    case "AUTH_CHANGED":
      if (action.payload.signedIn) {
        return {
          status: "signed_in",
          name: action.payload.name || "",
          provider: isAuthProvider(action.payload.provider) ? action.payload.provider : "google",
        };
      }
      return {
        status: "signed_out",
        error: null,
      };

    case "RESET_ERROR":
      if (state.status === "signed_out") {
        return {
          status: "signed_out",
          error: null,
        };
      }
      return state;

    default:
      return state;
  }
}

/**
 * The locale keys of the account section's two notice lines: `status` is read out politely
 * (waiting for the browser), `alert` at once (a failed sign-in). At most one is set.
 */
export interface AccountNotice {
  readonly status: string | null;
  readonly alert: string | null;
}

/** The notice lines for `state`. */
export function accountNotice(state: AccountState): AccountNotice {
  if (state.status === "signing_in") {
    return { status: "auth.waiting", alert: null };
  }
  if (state.status === "signed_out" && state.error) {
    return { status: null, alert: state.error };
  }
  return { status: null, alert: null };
}

export type AccountInvoke = (cmd: string, args?: Record<string, unknown>) => Promise<unknown>;
export type AccountSubscribe = (
  onEvent: (payload: unknown) => void,
) => Promise<() => void>;

export interface AccountStore {
  /** Current account state snapshot. */
  getState(): AccountState;
  /** Whether the store is backed by Tauri invoke (false outside Tauri). */
  canSignIn(): boolean;
  /** Subscribes to state changes. Returns an unsubscribe function. */
  subscribe(listener: (state: AccountState) => void): () => void;
  /** Loads current auth status from backend (via `invoke("auth_status")`). */
  load(): Promise<AccountState>;
  /** Starts sign in flow with the given provider (via `invoke("sign_in", { provider })`). */
  signIn(provider: AuthProvider): Promise<AccountState>;
  /** Signs out (via `invoke("sign_out")`). */
  signOut(): Promise<AccountState>;
  /** Applies an incoming `auth-changed` event payload. */
  handleAuthChanged(payload: unknown): AccountState;
}

/**
 * Creates an account store backed by `invoke("auth_status")`, `invoke("sign_in")`,
 * and `invoke("sign_out")`. Outside Tauri (`invokeFn === null`), sign in is disabled.
 */
export function createAccountStore(
  invokeFn: AccountInvoke | null = null,
  subscribeFn: AccountSubscribe | null = null,
): AccountStore {
  let state: AccountState = { ...INITIAL_ACCOUNT_STATE };
  const listeners = new Set<(state: AccountState) => void>();

  function notify(): void {
    const current = { ...state };
    for (const listener of listeners) {
      listener(current);
    }
  }

  function dispatch(action: AccountAction): AccountState {
    state = accountReducer(state, action);
    notify();
    return { ...state };
  }

  if (subscribeFn) {
    void subscribeFn((raw) => {
      if (isAuthStatus(raw)) {
        dispatch({ type: "AUTH_CHANGED", payload: raw });
      }
    }).catch(() => {
      // Event subscription unavailable outside Tauri or during tests.
    });
  }

  return {
    getState(): AccountState {
      return { ...state };
    },
    canSignIn(): boolean {
      return invokeFn !== null;
    },
    subscribe(listener: (state: AccountState) => void): () => void {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
    async load(): Promise<AccountState> {
      if (!invokeFn) {
        return { ...state };
      }
      try {
        const raw = await invokeFn("auth_status");
        if (isAuthStatus(raw)) {
          return dispatch({ type: "AUTH_CHANGED", payload: raw });
        }
      } catch {
        // Keep current state if auth_status fails
      }
      return { ...state };
    },
    async signIn(provider: AuthProvider): Promise<AccountState> {
      if (!invokeFn) {
        return { ...state };
      }
      dispatch({ type: "SIGN_IN_START", provider });
      try {
        const raw = await invokeFn("sign_in", { provider });
        const result = raw as SignInResult | null | undefined;
        if (result && typeof result.name === "string" && isAuthProvider(result.provider)) {
          return dispatch({
            type: "SIGN_IN_SUCCESS",
            name: result.name,
            provider: result.provider,
          });
        }
        return dispatch({ type: "SIGN_IN_ERROR", error: "auth.failed" });
      } catch (err: unknown) {
        // Rust rejects with a locale key; anything else is a plain failure.
        const errorKey = typeof err === "string" && AUTH_ERRORS.includes(err) ? err : "auth.failed";
        return dispatch({ type: "SIGN_IN_ERROR", error: errorKey });
      }
    },
    async signOut(): Promise<AccountState> {
      if (!invokeFn) {
        return { ...state };
      }
      try {
        await invokeFn("sign_out");
      } catch {
        // Sign out locally regardless of IPC failure
      }
      return dispatch({ type: "SIGN_OUT_SUCCESS" });
    },
    handleAuthChanged(payload: unknown): AccountState {
      if (isAuthStatus(payload)) {
        return dispatch({ type: "AUTH_CHANGED", payload });
      }
      return { ...state };
    },
  };
}
