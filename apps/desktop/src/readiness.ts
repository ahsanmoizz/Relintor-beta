export type ReadinessReader<T> = () => Promise<T>;

export type ReadinessGate<T> = {
  request: (reader: ReadinessReader<T>, force?: boolean) => Promise<T>;
  invalidate: () => void;
  inFlight: () => boolean;
};

export function createReadinessGate<T>(minIntervalMs = 2_000): ReadinessGate<T> {
  let active: Promise<T> | null = null;
  let lastStartedAt = 0;
  let latest: T | undefined;

  return {
    request(reader, force = false) {
      if (active) return active;
      const now = Date.now();
      if (!force && latest !== undefined && now - lastStartedAt < minIntervalMs) {
        return Promise.resolve(latest);
      }

      const request = reader()
        .then((value) => {
          latest = value;
          lastStartedAt = Date.now();
          return value;
        })
        .catch((error) => {
          latest = undefined;
          lastStartedAt = 0;
          throw error;
        })
        .finally(() => {
          if (active === request) active = null;
        });
      active = request;
      return request;
    },
    invalidate() {
      latest = undefined;
      lastStartedAt = 0;
    },
    inFlight() {
      return active !== null;
    },
  };
}
