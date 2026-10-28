import { useCallback, useEffect, useRef, useState } from "react";
import { api, type FlightMeta } from "../../api";
import { adjacentFlights, typeMedian, type TypeMedian } from "../../lib/history";
import { useFlights } from "../../lib/use-backend";

/** The flights before and after this one in time, from the flight list (newly saved flights included). */
export function useAdjacentFlights(id: string) {
  const { state } = useFlights();
  return state.status === "ready" ? adjacentFlights(state.data, id) : {};
}

/** The pilot's median touchdown in this type over their other flights; null without a type or with too few flights. */
export function useTypeMedian(type: string | undefined, id: string): TypeMedian | null {
  const [value, setValue] = useState<TypeMedian | null>(null);
  useEffect(() => {
    let gone = false;
    if (type) api.trends({ aircraft: type }).then((t) => { if (!gone) setValue(typeMedian(t.landings, type, id)); }, () => {});
    return () => { gone = true; setValue(null); };
  }, [type, id]);
  return value;
}

/** The flight's title, notes and tags. `save` shows a change at once and writes it; a failed write puts the last saved
 * value back and rejects, so the field can say why. */
export function useFlightMeta(id: string) {
  const [meta, setMeta] = useState<FlightMeta>({ tags: [] });
  const current = useRef(meta);
  const show = useCallback((value: FlightMeta) => { current.current = value; setMeta(value); }, []);
  useEffect(() => {
    let gone = false;
    api.flightMetaGet(id).then((value) => { if (!gone) show(value); }, () => {});
    return () => { gone = true; };
  }, [id, show]);
  const save = useCallback((change: Partial<FlightMeta>) => {
    const previous = current.current, next = { ...previous, ...change };
    show(next);
    return api.flightMetaSet(id, next).catch((error: unknown) => {
      if (current.current === next) show(previous);
      throw error;
    });
  }, [id, show]);
  return { meta, save };
}
