// API routes export one function per HTTP method and return a Response (or any JSON value).
export function GET(request, { searchParams }) {
  return { now: new Date().toISOString(), tz: searchParams.get("tz") ?? "UTC" };
}

export async function POST(request) {
  const body = await request.json();
  return Response.json({ echo: body }, { status: 201 });
}
