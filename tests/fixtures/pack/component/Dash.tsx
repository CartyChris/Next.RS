import { useState } from "react";
import { Theme, Card, CardContent, Stat, Button, Grid, Heading } from "forge:ui";

export default function Dash() {
  const [n, setN] = useState(3);
  return (
    <Theme accent="verdigris">
      <main style={{ padding: 24 }}>
        <Heading level={1}>Dash</Heading>
        <Grid columns={2}>
          <Card><CardContent><Stat label="Builds" value={n} /></CardContent></Card>
        </Grid>
        <Button id="more" onClick={() => setN(n + 1)}>More</Button>
      </main>
    </Theme>
  );
}
