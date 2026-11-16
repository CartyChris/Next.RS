// Every Forge UI component in one place, inside a nested Theme whose accent, radius and
// density you can change live. The browser tests drive this page under all three targets.
import {
  Accordion, Alert, Avatar, Badge, Bar, Breadcrumbs, Button, ButtonGroup, Card, CardContent, CardHeader,
  CardTitle, Code, Container, Drawer, EmptyState, Grid, Heading, Hud, HudSlot, Icon, IconButton, Image, Kbd,
  Menu, PageHeader, Pagination, Popover, Progress, RadioGroup, Row, SegmentedControl, Select, Skeleton, Slider,
  Spinner, Stack, Stat, Switch, Table, Text, TextInput, Theme, Toaster, Tooltip, toast, useDisclosure,
  type AccentPreset, type Column, type Density, type Radius,
} from "forge:ui";
import { useState } from "react";
import anvil from "../_assets/anvil.svg";

export const meta = {
  title: "Component gallery · Relay",
  description: "Every Forge UI component, with a live theme playground.",
};

const accents: AccentPreset[] = ["ember", "violet", "cobalt", "verdigris", "gold", "rose", "steel"];

interface Service {
  name: string;
  owner: string;
  latency: number;
  status: "Healthy" | "Degraded";
}

const services: Service[] = [
  { name: "Gateway", owner: "Platform", latency: 42, status: "Healthy" },
  { name: "Billing", owner: "Payments", latency: 118, status: "Degraded" },
  { name: "Search", owner: "Discovery", latency: 67, status: "Healthy" },
  { name: "Auth", owner: "Identity", latency: 23, status: "Healthy" },
];

const columns: Column<Service>[] = [
  { key: "name", header: "Service", sortable: true },
  { key: "owner", header: "Owner" },
  { key: "latency", header: "p95 latency", sortable: true, align: "end", cell: (row) => `${row.latency} ms` },
  {
    key: "status", header: "Status",
    cell: (row) => <Badge tone={row.status === "Healthy" ? "success" : "warning"} dot>{row.status}</Badge>,
  },
];

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <Card>
      <CardHeader><CardTitle>{title}</CardTitle></CardHeader>
      <CardContent><Stack>{children}</Stack></CardContent>
    </Card>
  );
}

export default function Gallery() {
  const [accent, setAccent] = useState<AccentPreset>("ember");
  const [custom, setCustom] = useState("");
  const [radius, setRadius] = useState<Radius>("md");
  const [density, setDensity] = useState<Density>("comfortable");
  const [saving, setSaving] = useState(false);
  const [live, setLive] = useState(true);
  const [page, setPage] = useState(1);
  const [health, setHealth] = useState(72);
  const drawer = useDisclosure();

  return (
    <main className="relay-main" id="main">
      <Theme theme="inherit" accent={custom || accent} radius={radius} density={density} id="playground">
        <Container size="wide">
          <Stack gap="lg">
            <PageHeader
              title="Component gallery"
              description="Change the accent, corners and density, and every component below follows."
              eyebrow={<Breadcrumbs items={[{ label: "Workspace", href: "/" }, { label: "Component gallery" }]} />}
              actions={<Button iconStart={<Icon name="spark" />} onClick={() => toast({ title: "Theme copied", tone: "success" })}>Copy theme</Button>}
            />

            <Section title="Theme">
              <Grid columns={2}>
                <Select label="Accent" value={accent} onChange={(event) => {
                  setCustom("");
                  setAccent(event.currentTarget.value as AccentPreset);
                }}>
                  {accents.map((name) => <option key={name} value={name}>{name}</option>)}
                </Select>
                <TextInput label="Custom accent" description="Any CSS color. Its hue becomes an accessible palette."
                  placeholder="#0a7f5a" value={custom} onChange={(event) => setCustom(event.currentTarget.value)} />
              </Grid>
              <Row gap="lg" align="end">
                <SegmentedControl name="radius" label="Corners" value={radius} onValueChange={(value) => setRadius(value as Radius)}
                  options={["none", "sm", "md", "lg", "full"].map((value) => ({ value, label: value }))} />
                <SegmentedControl name="density" label="Density" value={density} onValueChange={(value) => setDensity(value as Density)}
                  options={[{ value: "compact", label: "Compact" }, { value: "comfortable", label: "Comfortable" }, { value: "spacious", label: "Spacious" }]} />
              </Row>
            </Section>

            <Section title="Actions">
              <Row>
                <Button id="primary">Primary</Button>
                <Button variant="secondary">Secondary</Button>
                <Button variant="soft">Soft</Button>
                <Button variant="ghost">Ghost</Button>
                <Button variant="danger">Danger</Button>
                <Button variant="link">Link</Button>
              </Row>
              <Row>
                <Button id="save" loading={saving} onClick={() => {
                  setSaving(true);
                  setTimeout(() => {
                    setSaving(false);
                    toast({ title: "Changes saved", tone: "success" });
                  }, 400);
                }}>Save changes</Button>
                <ButtonGroup label="Zoom">
                  <Button variant="secondary" size="sm" aria-label="Zoom out"><Icon name="minus" /></Button>
                  <Button variant="secondary" size="sm">100%</Button>
                  <Button variant="secondary" size="sm" aria-label="Zoom in"><Icon name="plus" /></Button>
                </ButtonGroup>
                <Tooltip content="Copy the share link">
                  <IconButton label="Copy link" onClick={() => toast("Link copied")}><Icon name="copy" /></IconButton>
                </Tooltip>
                <Menu label="Release" items={[
                  { label: "Rename", icon: <Icon name="settings" />, onSelect: () => toast("Renamed") },
                  { label: "Duplicate", icon: <Icon name="copy" />, shortcut: "⌘D", onSelect: () => toast("Duplicated") },
                  { label: "Archive", disabled: true },
                  { separator: true },
                  { label: "Delete", danger: true, icon: <Icon name="trash" />, onSelect: () => toast({ title: "Deleted", tone: "danger" }) },
                ]} />
                <Popover label="Filters">
                  <Stack gap="sm">
                    <Heading level={3} size="sm">Filter services</Heading>
                    <Switch label="Only degraded" />
                    <Text size="sm" tone="muted">Press <Kbd>Esc</Kbd> to close.</Text>
                  </Stack>
                </Popover>
                <Button variant="secondary" onClick={drawer.onOpen}>Open details</Button>
              </Row>
            </Section>

            <Section title="Fields">
              <Grid columns={2}>
                <Stack>
                  <Switch label="Live updates" description="Refresh metrics as they arrive." checked={live}
                    onChange={(event) => setLive(event.currentTarget.checked)} />
                  <Slider label="Health" min={0} max={100} value={health} showValue
                    onChange={(event) => setHealth(event.currentTarget.valueAsNumber)} />
                </Stack>
                <RadioGroup name="channel" label="Release channel" defaultValue="stable" options={[
                  { value: "stable", label: "Stable", description: "Tested builds, monthly." },
                  { value: "beta", label: "Beta", description: "New features, weekly." },
                  { value: "nightly", label: "Nightly", disabled: true },
                ]} />
              </Grid>
            </Section>

            <Section title="Status and data">
              <Grid columns={4}>
                <Stat label="Deploys" value="128" change="+12%" trend="up" hint="vs last week" />
                <Stat label="Failures" value="3" change="-40%" trend="down" />
                <Stat label="Median build" value="42 s" />
                <Row><Avatar name="Ada Lovelace" /><Avatar name="Grace Hopper" size="sm" /><Spinner label="Syncing" /></Row>
              </Grid>
              <Progress label="Rollout" value={64} showValue />
              <Progress label="Indexing" />
              <Skeleton lines={3} />
              <Table caption="Services" columns={columns} rows={services} rowKey={(row) => row.name} striped />
              <Row justify="between">
                <Text size="sm" tone="muted">Page {page} of 9</Text>
                <Pagination page={page} pageCount={9} onPageChange={setPage} />
              </Row>
              <EmptyState title="No incidents" description="Everything has been quiet for 12 days."
                icon={<Icon name="check" size={20} />} action={<Button variant="secondary">View history</Button>} />
              <Alert tone="warning" title="Billing is degraded" actions={<Button size="sm" variant="secondary">Open runbook</Button>}>
                p95 latency is above the <Code>100 ms</Code> objective.
              </Alert>
            </Section>

            <Section title="Disclosure and media">
              <Grid columns={2}>
                <Accordion items={[
                  { value: "what", title: "What is Forge UI?", content: "A component library built into the forge binary." },
                  { value: "theme", title: "How do I theme it?", content: "Pass accent, radius and density to Theme, or override tokens." },
                  { value: "offline", title: "Does it work offline?", content: "Yes. Single-file builds inline the styles and scripts." },
                ]} />
                <Image src={anvil} alt="An anvil glowing in a dark forge" width={640} height={360} rounded priority />
              </Grid>
            </Section>

            <Section title="Game HUD">
              <div className="relay-stage">
                <Hud>
                  <HudSlot position="top-left" panel><Bar label="Health" value={health} tone={health < 30 ? "danger" : "success"} showValue /></HudSlot>
                  <HudSlot position="top-right" panel><Text size="sm"><strong>Score</strong> 12,480</Text></HudSlot>
                  <HudSlot position="bottom" interactive>
                    <Button size="sm" variant="secondary" iconStart={<Icon name="bolt" />} onClick={() => setHealth((value) => Math.max(0, value - 15))}>
                      Take a hit
                    </Button>
                  </HudSlot>
                </Hud>
              </div>
            </Section>
          </Stack>
        </Container>
        <Drawer open={drawer.open} onOpenChange={drawer.setOpen} title="Gateway" description="Edge routing for every region."
          footer={<Button onClick={drawer.onClose}>Done</Button>}>
          <Stack>
            <Stat label="p95 latency" value="42 ms" change="-8%" trend="down" />
            <Progress label="Error budget" value={88} tone="success" showValue />
          </Stack>
        </Drawer>
        <Toaster label="Gallery notifications" />
      </Theme>
    </main>
  );
}
