// Live lobby feed: rooms publish their status to the "lobby" topic; this socket subscribes.
import { listRooms } from "../../_game/registry";

export const socket: ForgeSocketHandlers = {
  open(ws) {
    ws.subscribe("lobby");
    ws.send({ t: "rooms", rooms: listRooms() });
  },
};

export const GET = () => ({ rooms: listRooms() });
