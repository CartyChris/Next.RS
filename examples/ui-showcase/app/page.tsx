import {
  Alert, Badge, Button, Card, CardContent, CardDescription, CardHeader, CardTitle,
  Checkbox, Container, Dialog, Grid, NotificationRegion, Select, Stack, Tabs,
  Textarea, TextInput,
} from "@nextrs/ui";
import { useId, useRef, useState, type FormEvent } from "react";

export const meta = {
  title: "Release overview · Relay",
  description: "A release workspace built with Forge UI.",
};

const environments = {
  production: "Production",
  staging: "Staging",
  preview: "Preview",
} as const;

type Environment = keyof typeof environments;
type ReleaseStatus = "Live" | "Ready" | "Scheduled";

interface Release {
  id: string;
  name: string;
  environment: Environment;
  status: ReleaseStatus;
  updated: string;
  notes?: string;
  notify?: boolean;
}

const releaseTones = { Live: "success", Ready: "info", Scheduled: "warning" } as const;

const initialReleases: Release[] = [
  { id: "web-248", name: "Web app 2.48", environment: "production", status: "Live", updated: "Today, 09:40" },
  { id: "api-164", name: "API gateway 1.64", environment: "staging", status: "Ready", updated: "Today, 09:15" },
  { id: "docs-312", name: "Docs site 3.12", environment: "preview", status: "Live", updated: "Yesterday" },
  { id: "worker-82", name: "Event worker 8.2", environment: "production", status: "Scheduled", updated: "Yesterday" },
];

const weeklyDeploys = [
  { day: "Mon", count: 3 },
  { day: "Tue", count: 5 },
  { day: "Wed", count: 4 },
  { day: "Thu", count: 7 },
  { day: "Fri", count: 6 },
  { day: "Sat", count: 2 },
  { day: "Sun", count: 4 },
];

function isEnvironment(value: string): value is Environment {
  return Object.hasOwn(environments, value);
}

function DeploymentVolume() {
  const total = weeklyDeploys.reduce((sum, day) => sum + day.count, 0);

  return (
    <Card className="relay-volume">
      <CardHeader className="relay-volume-header">
        <div>
          <CardTitle>Deployment volume</CardTitle>
          <CardDescription>Completed deployments in the last 7 days</CardDescription>
        </div>
        <div className="relay-volume-total">
          <strong>{total}</strong> <span>deployments</span>
        </div>
      </CardHeader>
      <CardContent>
        <ol className="relay-chart" aria-label="Deployments by day">
          {weeklyDeploys.map(({ day, count }) => (
            <li key={day} className="relay-chart-column">
              <div className="relay-chart-track">
                <span className="relay-chart-count">{count}</span>
                <span className={`relay-chart-bar relay-chart-bar--${count}`} aria-hidden="true" />
              </div>
              <span className="relay-chart-day">{day}</span>
            </li>
          ))}
        </ol>
      </CardContent>
    </Card>
  );
}

function ReleaseQueue({
  releases, filter, onFilterChange,
}: {
  releases: Release[];
  filter: Environment | "all";
  onFilterChange: (filter: Environment | "all") => void;
}) {
  const visible = releases.filter((release) => filter === "all" || release.environment === filter);

  return (
    <Card>
      <CardHeader className="relay-table-header">
        <div>
          <CardTitle>Release queue</CardTitle>
          <CardDescription>Track changes across your environments.</CardDescription>
        </div>
        <Select
          label="Filter environment"
          className="relay-filter"
          value={filter}
          onChange={({ currentTarget }) => {
            const next = currentTarget.value;
            if (next === "all" || isEnvironment(next)) onFilterChange(next);
          }}
        >
          <option value="all">All environments</option>
          {Object.entries(environments).map(([value, label]) => (
            <option key={value} value={value}>{label}</option>
          ))}
        </Select>
      </CardHeader>
      <CardContent className="relay-table-content">
        {visible.length ? (
          <table className="relay-table" aria-label="Releases">
            <thead>
              <tr><th scope="col">Release</th><th scope="col">Environment</th><th scope="col">Status</th></tr>
            </thead>
            <tbody>
              {visible.map((release) => (
                <tr key={release.id}>
                  <th scope="row">
                    <span className="relay-release-name">{release.name}</span>
                    <span className="relay-release-time">{release.updated}</span>
                  </th>
                  <td>{environments[release.environment]}</td>
                  <td><Badge tone={releaseTones[release.status]}>{release.status}</Badge></td>
                </tr>
              ))}
            </tbody>
          </table>
        ) : (
          <Alert title="No releases in this environment">Choose another environment to see its releases.</Alert>
        )}
      </CardContent>
    </Card>
  );
}

function ReleaseActivity({ releases }: { releases: Release[] }) {
  return (
    <Card>
      <CardHeader>
        <CardTitle>Release activity</CardTitle>
        <CardDescription>Latest status for each release in this workspace.</CardDescription>
      </CardHeader>
      <CardContent>
        <ul className="relay-activity">
          {releases.map((release) => (
            <li key={release.id}>
              <div className="relay-activity-time"><span aria-hidden="true" />{release.updated}</div>
              <div>
                <strong>{release.name}</strong>
                <p>{release.status} · {environments[release.environment]}</p>
                {release.notes && <p className="relay-activity-notes">{release.notes}</p>}
                {release.notify && <p>Team notification requested</p>}
              </div>
            </li>
          ))}
        </ul>
      </CardContent>
    </Card>
  );
}

function ScheduleDialog({
  open, onOpenChange, onSchedule,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onSchedule: (release: Omit<Release, "id" | "updated" | "status">) => void;
}) {
  const formId = useId();
  const [name, setName] = useState("");
  const [environment, setEnvironment] = useState<Environment>("staging");
  const [notes, setNotes] = useState("");
  const [notify, setNotify] = useState(false);
  const [error, setError] = useState("");

  function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const cleanName = name.trim();
    if (!cleanName) {
      setError("Enter a release name.");
      const field = event.currentTarget.elements.namedItem("releaseName");
      if (field instanceof HTMLInputElement) field.focus();
      return;
    }

    onSchedule({ name: cleanName, environment, notes: notes.trim(), notify });
  }

  return (
    <Dialog
      open={open}
      onOpenChange={onOpenChange}
      title="Schedule a release"
      description="Add an entry to the demo queue. No deployment or message will be sent."
      footer={
        <>
          <Button variant="secondary" onClick={() => onOpenChange(false)}>Cancel</Button>
          <Button type="submit" form={formId}>Schedule release</Button>
        </>
      }
    >
      <form id={formId} onSubmit={submit} noValidate>
        <Stack gap="md">
          <TextInput
            name="releaseName"
            label="Release name"
            value={name}
            error={error}
            placeholder="Web app 2.49"
            required
            maxLength={60}
            onChange={({ currentTarget }) => {
              setName(currentTarget.value);
              if (error && currentTarget.value.trim()) setError("");
            }}
          />
          <Select
            label="Environment"
            value={environment}
            onChange={({ currentTarget }) => {
              if (isEnvironment(currentTarget.value)) setEnvironment(currentTarget.value);
            }}
          >
            {Object.entries(environments).map(([value, label]) => (
              <option key={value} value={value}>{label}</option>
            ))}
          </Select>
          <Textarea
            label="Release notes"
            description="Optional"
            value={notes}
            rows={3}
            maxLength={280}
            onChange={({ currentTarget }) => setNotes(currentTarget.value)}
          />
          <Checkbox
            label="Notify the release team"
            description="Records the preference in demo activity."
            checked={notify}
            onChange={({ currentTarget }) => setNotify(currentTarget.checked)}
          />
        </Stack>
      </form>
    </Dialog>
  );
}

export default function ReleaseOverview() {
  const newReleaseButton = useRef<HTMLButtonElement>(null);
  const [releases, setReleases] = useState(initialReleases);
  const [filter, setFilter] = useState<Environment | "all">("all");
  const [open, setOpen] = useState(false);
  const [formVersion, setFormVersion] = useState(0);
  const [tab, setTab] = useState("releases");
  const [notification, setNotification] = useState<{ id: string; title: string; description: string; tone: "success" }>();
  const scheduled = releases.filter(({ status }) => status === "Scheduled").length;

  function schedule(release: Omit<Release, "id" | "updated" | "status">) {
    const id = `demo-${formVersion}`;
    setReleases((current) => [{ ...release, id, updated: "Just now", status: "Scheduled" }, ...current]);
    setOpen(false);
    setTab("releases");
    setFilter("all");
    setNotification({
      id,
      title: "Release scheduled",
      description: `${release.name} was added to the ${environments[release.environment]} queue.`,
      tone: "success",
    });
  }

  return (
    <main className="relay-main" id="main">
      <Container size="wide">
        <section className="relay-hero" aria-labelledby="relay-title">
          <div className="relay-hero-copy">
            <h1 id="relay-title">Release overview</h1>
            <p>Plan your next deployment, check release health, and follow changes across environments.</p>
          </div>
          <Button ref={newReleaseButton} className="relay-new-release" size="lg" onClick={() => {
            setFormVersion((value) => value + 1);
            setOpen(true);
          }}>
            <span aria-hidden="true">+</span> New release
          </Button>
        </section>

        <Grid columns={3} gap="md" className="relay-metrics">
          <Card className="relay-metric">
            <CardContent>
              <p>Successful deployments</p>
              <strong>98.7%</strong>
              <small className="relay-metric-success">↑ 1.2 points vs. prior week</small>
            </CardContent>
          </Card>
          <Card className="relay-metric">
            <CardContent>
              <p>Scheduled releases</p>
              <strong>{scheduled}</strong>
              <small>Across all environments</small>
            </CardContent>
          </Card>
          <Card className="relay-metric">
            <CardContent>
              <p>Median deploy time</p>
              <strong>4m 12s</strong>
              <small>Last 7 days</small>
            </CardContent>
          </Card>
        </Grid>

        <div className="relay-workspace">
          <DeploymentVolume />
          <Card className="relay-environments">
            <CardHeader>
              <CardTitle>Environment health</CardTitle>
              <CardDescription>Sample service status</CardDescription>
            </CardHeader>
            <CardContent>
              <ul>
                <li><span>Production</span><Badge tone="success">Healthy</Badge></li>
                <li><span>Staging</span><Badge tone="success">Healthy</Badge></li>
                <li><span>Preview</span><Badge tone="warning">Maintenance</Badge></li>
              </ul>
            </CardContent>
          </Card>
        </div>

        <Tabs
          label="Release workspace"
          value={tab}
          onValueChange={setTab}
          items={[
            { value: "releases", label: "Releases", content: <ReleaseQueue releases={releases} filter={filter} onFilterChange={setFilter} /> },
            { value: "activity", label: "Activity", content: <ReleaseActivity releases={releases} /> },
          ]}
        />
        <footer className="relay-page-footer">Demo workspace · Changes last until you leave this page.</footer>
      </Container>
      <ScheduleDialog key={formVersion} open={open} onOpenChange={setOpen} onSchedule={schedule} />
      <NotificationRegion
        notifications={notification ? [notification] : []}
        onDismiss={() => {
          newReleaseButton.current?.focus();
          setNotification(undefined);
        }}
      />
    </main>
  );
}
