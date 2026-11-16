import {
  Button, Card, CardContent, CardHeader, CardTitle, Container, PageHeader, RadioGroup, Stack, Switch, TextInput, toast,
} from "forge:ui";
import { usePersistentState } from "forge:store";
import { useState, type FormEvent } from "react";

export const meta = { title: "Settings · Starter" };

export default function Settings() {
  // Saved in localStorage, so it survives reloads (also in a single HTML file).
  const [name, setName] = usePersistentState("starter:name", "Acme");
  const [error, setError] = useState("");

  function save(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const value = String(new FormData(event.currentTarget).get("name") ?? "").trim();
    if (value.length < 2) return setError("Use at least 2 characters.");
    setError("");
    setName(value);
    toast({ title: "Settings saved", tone: "success" });
  }

  return (
    <main className="starter-page">
      <Container size="narrow">
        <Stack gap="lg">
          <PageHeader title="Settings" description="These are stored in this browser." />
          <Card>
            <CardHeader><CardTitle>Workspace</CardTitle></CardHeader>
            <CardContent>
              <form onSubmit={save} noValidate>
                <Stack>
                  <TextInput name="name" label="Workspace name" defaultValue={name} key={name} required error={error} />
                  <RadioGroup name="digest" label="Email digest" defaultValue="weekly" options={[
                    { value: "daily", label: "Daily" },
                    { value: "weekly", label: "Weekly" },
                    { value: "never", label: "Never" },
                  ]} />
                  <Switch name="beta" label="Try beta features" />
                  <div><Button type="submit">Save settings</Button></div>
                </Stack>
              </form>
            </CardContent>
          </Card>
        </Stack>
      </Container>
    </main>
  );
}
