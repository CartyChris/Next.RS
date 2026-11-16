import {
  Badge, Button, Card, CardContent, Container, EmptyState, Grid, Icon, PageHeader, Stack, Stat, Table, TextInput,
  toast, type Column,
} from "forge:ui";
import { useData } from "forge:data";
import { useState } from "react";
import type { Project } from "./_data";

export const meta = { title: "Projects · Starter", description: "A Next.RS starter built with forge:ui." };

const tones = { Live: "success", Building: "info", Paused: "warning" } as const;

const columns: Column<Project>[] = [
  { key: "name", header: "Project", sortable: true },
  { key: "owner", header: "Owner", sortable: true },
  { key: "status", header: "Status", cell: (row) => <Badge tone={tones[row.status]} dot>{row.status}</Badge> },
  { key: "visitors", header: "Visitors", sortable: true, align: "end", cell: (row) => row.visitors.toLocaleString("en-US") },
];

export default function Projects() {
  const { projects } = useData<{ projects: Project[] }>();
  const [query, setQuery] = useState("");
  const shown = projects.filter((project) => project.name.toLowerCase().includes(query.trim().toLowerCase()));
  const visitors = projects.reduce((sum, project) => sum + project.visitors, 0);

  return (
    <main className="starter-page">
      <Container>
        <Stack gap="lg">
          <PageHeader
            title="Projects"
            description="Everything your team has shipped."
            actions={<Button iconStart={<Icon name="plus" />} onClick={() => toast({ title: "Project created", tone: "success" })}>New project</Button>}
          />
          <Grid columns={3}>
            <Card variant="spotlight"><CardContent><Stat label="Projects" value={projects.length} /></CardContent></Card>
            <Card><CardContent><Stat label="Live" value={projects.filter((p) => p.status === "Live").length} change="+1" trend="up" hint="this week" /></CardContent></Card>
            <Card><CardContent><Stat label="Visitors" value={visitors.toLocaleString("en-US")} /></CardContent></Card>
          </Grid>
          <TextInput label="Filter projects" type="search" placeholder="Type a name" value={query}
            onChange={(event) => setQuery(event.currentTarget.value)} />
          <Table caption="All projects" columns={columns} rows={shown} rowKey={(row) => row.id} hideCaption
            empty={<EmptyState title="No projects match" description="Try a different name." />} />
        </Stack>
      </Container>
    </main>
  );
}
