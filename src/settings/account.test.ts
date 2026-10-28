// Run with `npm test` (Node's own test runner; Node 24 strips the types).
// Unit tests for the pure account state machine (src/settings/account.ts, WP67b).
import assert from "node:assert/strict";
import { test } from "node:test";

import {
  AUTH_PROVIDERS,
  INITIAL_ACCOUNT_STATE,
  accountReducer,
  createAccountStore,
  isAuthProvider,
  isAuthStatus,
} from "./account.ts";
import type {
  AccountAction,
  AccountInvoke,
  AccountState,
  AccountSubscribe,
  AuthStatus,
} from "./account.ts";

test("isAuthProvider validates only supported providers", () => {
  assert.equal(isAuthProvider("google"), true);
  assert.equal(isAuthProvider("github"), true);
  assert.equal(isAuthProvider("apple"), false);
  assert.equal(isAuthProvider(""), false);
  assert.equal(isAuthProvider(null), false);
  assert.equal(isAuthProvider(undefined), false);
  assert.equal(isAuthProvider(123), false);
});

test("isAuthStatus validates auth status payloads", () => {
  assert.equal(isAuthStatus({ signedIn: false }), true);
  assert.equal(isAuthStatus({ signedIn: true, name: "Alice", provider: "google" }), true);
  assert.equal(isAuthStatus({ signedIn: true, name: "Bob", provider: "github" }), true);

  assert.equal(isAuthStatus(null), false);
  assert.equal(isAuthStatus(undefined), false);
  assert.equal(isAuthStatus("string"), false);
  assert.equal(isAuthStatus({ signedIn: "not-a-bool" }), false);
  assert.equal(isAuthStatus({ signedIn: true, provider: "invalid" }), false);
  assert.equal(isAuthStatus({ signedIn: true, name: 123 }), false);
});

test("accountReducer handles initial signed_out state", () => {
  assert.deepEqual(INITIAL_ACCOUNT_STATE, {
    status: "signed_out",
    error: null,
  });
});

test("accountReducer transitions signed out -> signing in -> signed in", () => {
  const startAction: AccountAction = {
    type: "SIGN_IN_START",
    provider: "google",
  };
  const signingIn = accountReducer(INITIAL_ACCOUNT_STATE, startAction);
  assert.deepEqual(signingIn, {
    status: "signing_in",
    provider: "google",
  });

  const successAction: AccountAction = {
    type: "SIGN_IN_SUCCESS",
    name: "Alex",
    provider: "google",
  };
  const signedIn = accountReducer(signingIn, successAction);
  assert.deepEqual(signedIn, {
    status: "signed_in",
    name: "Alex",
    provider: "google",
  });
});

test("accountReducer transitions signing in -> failed with error key", () => {
  const signingIn: AccountState = {
    status: "signing_in",
    provider: "github",
  };
  const errorAction: AccountAction = {
    type: "SIGN_IN_ERROR",
    error: "auth.cancelled",
  };
  const failed = accountReducer(signingIn, errorAction);
  assert.deepEqual(failed, {
    status: "signed_out",
    error: "auth.cancelled",
  });
});

test("accountReducer transitions signed in -> sign out", () => {
  const signedIn: AccountState = {
    status: "signed_in",
    name: "Alex",
    provider: "google",
  };
  const signOutAction: AccountAction = {
    type: "SIGN_OUT_SUCCESS",
  };
  const signedOut = accountReducer(signedIn, signOutAction);
  assert.deepEqual(signedOut, {
    status: "signed_out",
    error: null,
  });
});

test("accountReducer handles external auth-changed event", () => {
  // auth-changed while signed out -> signed in
  const signedInPayload: AuthStatus = {
    signedIn: true,
    name: "Sam",
    provider: "github",
  };
  const state1 = accountReducer(INITIAL_ACCOUNT_STATE, {
    type: "AUTH_CHANGED",
    payload: signedInPayload,
  });
  assert.deepEqual(state1, {
    status: "signed_in",
    name: "Sam",
    provider: "github",
  });

  // auth-changed while signed in -> signed out
  const state2 = accountReducer(state1, {
    type: "AUTH_CHANGED",
    payload: { signedIn: false },
  });
  assert.deepEqual(state2, {
    status: "signed_out",
    error: null,
  });

  // auth-changed while signing in -> signed in from elsewhere
  const signingInState: AccountState = {
    status: "signing_in",
    provider: "google",
  };
  const state3 = accountReducer(signingInState, {
    type: "AUTH_CHANGED",
    payload: { signedIn: true, name: "Taylor", provider: "google" },
  });
  assert.deepEqual(state3, {
    status: "signed_in",
    name: "Taylor",
    provider: "google",
  });
});

test("accountReducer handles RESET_ERROR", () => {
  const withError: AccountState = {
    status: "signed_out",
    error: "auth.failed",
  };
  const cleared = accountReducer(withError, { type: "RESET_ERROR" });
  assert.deepEqual(cleared, {
    status: "signed_out",
    error: null,
  });

  const signedIn: AccountState = {
    status: "signed_in",
    name: "Alex",
    provider: "google",
  };
  assert.equal(accountReducer(signedIn, { type: "RESET_ERROR" }), signedIn);
});

test("createAccountStore outside Tauri keeps sign in disabled", async () => {
  const store = createAccountStore(null);
  assert.equal(store.canSignIn(), false);
  assert.deepEqual(store.getState(), { status: "signed_out", error: null });

  // Calling signIn outside Tauri is a no-op
  const state = await store.signIn("google");
  assert.deepEqual(state, { status: "signed_out", error: null });

  const loaded = await store.load();
  assert.deepEqual(loaded, { status: "signed_out", error: null });
});

test("createAccountStore handles successful sign_in flow", async () => {
  const calls: { cmd: string; args?: Record<string, unknown> }[] = [];
  const invoke: AccountInvoke = async (cmd, args) => {
    calls.push({ cmd, args });
    if (cmd === "sign_in") {
      return { name: "Casey", provider: args?.provider };
    }
    throw new Error(`unexpected command ${cmd}`);
  };

  const store = createAccountStore(invoke);
  assert.equal(store.canSignIn(), true);

  const states: AccountState[] = [];
  store.subscribe((s) => states.push(s));

  const result = await store.signIn("google");
  assert.deepEqual(result, {
    status: "signed_in",
    name: "Casey",
    provider: "google",
  });
  assert.deepEqual(store.getState(), result);
  assert.deepEqual(calls, [{ cmd: "sign_in", args: { provider: "google" } }]);

  // Observed state sequence: signing_in, then signed_in
  assert.equal(states.length, 2);
  assert.equal(states[0].status, "signing_in");
  assert.equal(states[1].status, "signed_in");
});

test("createAccountStore handles sign_in rejection with locale error key", async () => {
  const invoke: AccountInvoke = async (cmd) => {
    if (cmd === "sign_in") {
      throw "auth.cancelled";
    }
    throw new Error(`unexpected command ${cmd}`);
  };

  const store = createAccountStore(invoke);
  const result = await store.signIn("github");
  assert.deepEqual(result, {
    status: "signed_out",
    error: "auth.cancelled",
  });
  assert.deepEqual(store.getState(), {
    status: "signed_out",
    error: "auth.cancelled",
  });
});

test("createAccountStore handles sign_in rejection with unavailable error", async () => {
  const invoke: AccountInvoke = async (cmd) => {
    if (cmd === "sign_in") {
      throw "auth.unavailable";
    }
    throw new Error(`unexpected command ${cmd}`);
  };

  const store = createAccountStore(invoke);
  const result = await store.signIn("google");
  assert.deepEqual(result, {
    status: "signed_out",
    error: "auth.unavailable",
  });
});

test("createAccountStore handles sign_out", async () => {
  let signedOutCalled = false;
  const invoke: AccountInvoke = async (cmd) => {
    if (cmd === "sign_in") {
      return { name: "Jordan", provider: "google" };
    }
    if (cmd === "sign_out") {
      signedOutCalled = true;
      return;
    }
    throw new Error(`unexpected command ${cmd}`);
  };

  const store = createAccountStore(invoke);
  await store.signIn("google");
  assert.equal(store.getState().status, "signed_in");

  const state = await store.signOut();
  assert.equal(signedOutCalled, true);
  assert.deepEqual(state, {
    status: "signed_out",
    error: null,
  });
});

test("createAccountStore load initializes signed in state when active", async () => {
  const invoke: AccountInvoke = async (cmd) => {
    if (cmd === "auth_status") {
      return { signedIn: true, name: "Morgan", provider: "github" };
    }
    throw new Error(`unexpected command ${cmd}`);
  };

  const store = createAccountStore(invoke);
  const state = await store.load();
  assert.deepEqual(state, {
    status: "signed_in",
    name: "Morgan",
    provider: "github",
  });
});

test("createAccountStore integrates auth-changed subscription", async () => {
  let subscriber: ((payload: unknown) => void) | null = null;
  const subscribeFn: AccountSubscribe = async (onEvent) => {
    subscriber = onEvent;
    return () => {
      subscriber = null;
    };
  };

  const store = createAccountStore(null, subscribeFn);
  assert.notEqual(subscriber, null);

  const notifications: AccountState[] = [];
  store.subscribe((s) => notifications.push(s));

  // External auth-changed event arrives
  subscriber!({
    signedIn: true,
    name: "Remote User",
    provider: "google",
  });

  assert.deepEqual(store.getState(), {
    status: "signed_in",
    name: "Remote User",
    provider: "google",
  });
  assert.equal(notifications.length, 1);
  assert.deepEqual(notifications[0], {
    status: "signed_in",
    name: "Remote User",
    provider: "google",
  });

  // External auth-changed sign out arrives
  subscriber!({ signedIn: false });
  assert.deepEqual(store.getState(), {
    status: "signed_out",
    error: null,
  });
  assert.equal(notifications.length, 2);
});
