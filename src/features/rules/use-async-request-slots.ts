import { useCallback, useEffect, useRef, useState } from "react";

export function useAsyncRequestSlots(prefix: string) {
  const generations = useRef(new Map<string, number>());
  const activeKeys = useRef(new Set<string>());
  const mounted = useRef(true);
  const [pending, setPending] = useState(false);

  useEffect(() => () => {
    mounted.current = false;
    activeKeys.current.forEach((key) => {
      generations.current.set(key, (generations.current.get(key) ?? 0) + 1);
    });
    activeKeys.current.clear();
  }, []);

  const runAsync = useCallback(async <T,>(
    slot: string,
    request: () => Promise<T>,
    apply: (value: T) => void,
    reject: (reason: unknown) => void,
  ) => {
    const key = `${prefix}:${slot}`;
    const generation = (generations.current.get(key) ?? 0) + 1;
    generations.current.set(key, generation);
    activeKeys.current.add(key);
    setPending(true);
    try {
      const value = await request();
      if (!mounted.current || generations.current.get(key) !== generation) return;
      apply(value);
    } catch (reason) {
      if (!mounted.current || generations.current.get(key) !== generation) return;
      reject(reason);
    } finally {
      if (!mounted.current || generations.current.get(key) !== generation) return;
      activeKeys.current.delete(key);
      setPending(activeKeys.current.size > 0);
    }
  }, [prefix]);

  return { pending, runAsync };
}
