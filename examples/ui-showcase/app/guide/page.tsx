import {
  Alert, Badge, ButtonLink, Card, CardContent, CardDescription, CardHeader,
  CardTitle, Container, Grid, Stack,
} from "forge:ui";
import { href } from "forge:navigation";

export const meta = {
  title: "Component guide · Relay",
  description: "See how Relay composes Forge UI components.",
};

const example = `import { Button, Card, CardContent, Theme } from "forge:ui";

export default function StatusCard() {
  return (
    <Theme theme="system">
      <Card>
        <CardContent>
          <Button type="button">New release</Button>
        </CardContent>
      </Card>
    </Theme>
  );
}`;

export default function ComponentGuide() {
  return (
    <main className="relay-main relay-guide" id="main">
      <Container size="wide">
        <section className="relay-hero" aria-labelledby="relay-guide-title">
          <div className="relay-hero-copy">
            <h1 id="relay-guide-title">Component guide</h1>
            <p>See which parts of Relay come from Forge UI and how to use them in your own app.</p>
          </div>
          <ButtonLink href={href("/")} className="relay-new-release" size="lg">Back to overview</ButtonLink>
        </section>
        <Stack gap="lg">
          <Grid columns={3} gap="md">
            <Card>
              <CardHeader>
                <Badge tone="info">Layout</Badge>
                <CardTitle>Build a screen</CardTitle>
                <CardDescription>Use Theme, Container, Grid, and Card for responsive surfaces.</CardDescription>
              </CardHeader>
            </Card>
            <Card>
              <CardHeader>
                <Badge tone="info">Forms</Badge>
                <CardTitle>Collect feedback</CardTitle>
                <CardDescription>Pair TextInput, Select, and Checkbox with native form submission.</CardDescription>
              </CardHeader>
            </Card>
            <Card>
              <CardHeader>
                <Badge tone="info">Interactions</Badge>
                <CardTitle>Guide the workflow</CardTitle>
                <CardDescription>Use Dialog, Tabs, Alert, and NotificationRegion to manage interaction and feedback.</CardDescription>
              </CardHeader>
            </Card>
          </Grid>
          <Card>
            <CardHeader>
              <CardTitle>Start with a card</CardTitle>
              <CardDescription>Import the components and include the stylesheet once in your root layout.</CardDescription>
            </CardHeader>
            <CardContent>
              <pre className="relay-code" tabIndex={0}><code>{example}</code></pre>
            </CardContent>
          </Card>
          <Alert tone="info" title="Choose a Forge build target">
            Run <code>forge build</code> for SSR, <code>forge build --target static</code> for a static site,
            or <code>forge build --target single-html</code> to open the app offline.
          </Alert>
        </Stack>
      </Container>
    </main>
  );
}
