type GoEnvInput = Readonly<{
  goModCache?: string;
  goroot: string;
  probeTmp: string;
}>;

type ModCacheInput = Readonly<{
  goModCache?: string;
  probeTmp: string;
}>;

function resolveModCache({ goModCache, probeTmp }: ModCacheInput): string {
  if (goModCache !== undefined && goModCache.length > 0) {
    return goModCache;
  }

  return `${probeTmp}/gomodcache`;
}

/** Build isolated Go environment variables with network proxies and cgo disabled. */
export function buildGoEnv({
  goModCache,
  goroot,
  probeTmp,
}: GoEnvInput): Readonly<Record<string, string>> {
  const modCache = resolveModCache({ goModCache, probeTmp });

  return {
    CGO_ENABLED: '0',
    GOCACHE: `${probeTmp}/gocache`,
    GOMODCACHE: modCache,
    GOPATH: `${probeTmp}/gopath`,
    GOPROXY: 'off',
    GOROOT: goroot,
    GOSUMDB: 'off',
  };
}
