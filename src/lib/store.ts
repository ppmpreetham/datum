import { Fragment, createElement, useId, useLayoutEffect, useSyncExternalStore } from "react";
import type { ReactNode } from "react";

export type Store<T> = {
  get: () => T;
  set: (patch: Partial<T> | ((state: T) => Partial<T>)) => void;
  subscribe: (listener: () => void) => () => void;
};

export function createStore<T extends object>(initial: T): Store<T> {
  let state = initial;
  const listeners = new Set<() => void>();
  return {
    get: () => state,
    set: (patch) => {
      const next = typeof patch === "function" ? patch(state) : patch;
      state = { ...state, ...next };
      listeners.forEach((listener) => listener());
    },
    subscribe: (listener) => {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
  };
}

export function useStore<T extends object, U>(store: Store<T>, selector: (state: T) => U): U {
  return useSyncExternalStore(store.subscribe, () => selector(store.get()));
}

/**
 * Lets a step component that lives in the DOM tree render 3D content inside
 * the shared <Canvas>, so one component owns both its HUD and its scene.
 */
export function createTunnel() {
  const store = createStore<{ nodes: [string, ReactNode][] }>({ nodes: [] });

  function In({ children }: { children: ReactNode }) {
    const id = useId();
    useLayoutEffect(() => {
      store.set(({ nodes }) => {
        const index = nodes.findIndex(([key]) => key === id);
        const next = [...nodes];
        if (index === -1) next.push([id, children]);
        else next[index] = [id, children];
        return { nodes: next };
      });
    }, [id, children]);
    useLayoutEffect(
      () => () => store.set(({ nodes }) => ({ nodes: nodes.filter(([key]) => key !== id) })),
      [id],
    );
    return null;
  }

  function Out() {
    const nodes = useStore(store, (state) => state.nodes);
    return createElement(
      Fragment,
      null,
      nodes.map(([key, node]) => createElement(Fragment, { key }, node)),
    );
  }

  return { In, Out };
}
