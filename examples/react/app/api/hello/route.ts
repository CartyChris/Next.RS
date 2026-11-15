export function GET(_request: Request, { searchParams }: ForgeContext) {
  return { hello: searchParams.get("name") ?? "world" };
}
