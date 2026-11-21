type GoEnvInput = Readonly<{
  goroot: string;
  probeTmp: string;
}>;

/** Build isolated Go environment variables with network proxies and cgo disabled. */
export function buildGoEnv({ goroot, probeTmp }: GoEnvInput): Readonly<Record<string, string>> {
  return {
    CGO_ENABLED: '0',
    GOCACHE: `${probeTmp}/gocache`,
    GOMODCACHE: `${probeTmp}/gomodcache`,
    GOPATH: `${probeTmp}/gopath`,
    GOPROXY: 'off',
    GOROOT: goroot,
    GOSUMDB: 'off',
  };
}
