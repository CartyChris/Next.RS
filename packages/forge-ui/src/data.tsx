import { useMemo, useState } from "react";
import type { ReactNode } from "react";
import { cx } from "./config";
import { Icon } from "./icon";

export interface Column<Row> {
  /** Unique per table; also the default field read from each row. */
  key: string;
  header: ReactNode;
  /** Custom cell content. Defaults to `row[key]`. */
  cell?: (row: Row) => ReactNode;
  /** Let people sort by this column. Sorts by `sortValue`, else by `row[key]`. */
  sortable?: boolean;
  sortValue?: (row: Row) => string | number;
  align?: "start" | "end";
  width?: string;
}

export interface TableSort {
  key: string;
  direction: "ascending" | "descending";
}

export interface TableProps<Row> {
  /** Describes the table; shown above it and used as its accessible name. */
  caption: string;
  columns: ReadonlyArray<Column<Row>>;
  rows: readonly Row[];
  /** A stable key per row. Defaults to the row index. */
  rowKey?: (row: Row, index: number) => string | number;
  defaultSort?: TableSort;
  /** Shown instead of rows when there are none. */
  empty?: ReactNode;
  striped?: boolean;
  hideCaption?: boolean;
  className?: string;
}

/** A data table with optional click-to-sort columns. Scrolls sideways on narrow screens. */
export function Table<Row>({
  caption, columns, rows, rowKey, defaultSort, empty = "Nothing to show yet.", striped, hideCaption, className,
}: TableProps<Row>) {
  const [sort, setSort] = useState<TableSort | undefined>(defaultSort);
  const sorted = useMemo(() => {
    const column = sort && columns.find((c) => c.key === sort.key);
    if (!sort || !column) return rows;
    const value = column.sortValue ?? ((row: Row) => (row as Record<string, unknown>)[column.key] as string | number);
    const sign = sort.direction === "ascending" ? 1 : -1;
    return [...rows].sort((a, b) => {
      const [x, y] = [value(a), value(b)];
      if (typeof x === "number" && typeof y === "number") return (x - y) * sign;
      return String(x).localeCompare(String(y), undefined, { numeric: true }) * sign;
    });
  }, [rows, columns, sort]);

  const toggle = (key: string) =>
    setSort((current) => current?.key === key && current.direction === "ascending"
      ? { key, direction: "descending" }
      : { key, direction: "ascending" });

  return (
    <div className={cx("forge-ui-table-wrap", className)} role="region" aria-label={caption} tabIndex={0}>
      <table className="forge-ui-table" data-striped={striped || undefined}>
        <caption className={hideCaption ? "forge-ui-visually-hidden" : undefined}>{caption}</caption>
        <thead>
          <tr>
            {columns.map((column) => {
              const active = sort?.key === column.key ? sort.direction : undefined;
              return (
                <th key={column.key} scope="col" data-align={column.align} style={column.width ? { width: column.width } : undefined}
                  aria-sort={column.sortable ? active ?? "none" : undefined}>
                  {column.sortable ? (
                    <button type="button" className="forge-ui-table-sort" onClick={() => toggle(column.key)}>
                      {column.header}
                      <Icon name={active === "descending" ? "arrow-down" : "arrow-up"} size={12}
                        className={active ? undefined : "forge-ui-table-sort-idle"} />
                    </button>
                  ) : column.header}
                </th>
              );
            })}
          </tr>
        </thead>
        <tbody>
          {sorted.length === 0 ? (
            <tr><td colSpan={columns.length} className="forge-ui-table-empty">{empty}</td></tr>
          ) : sorted.map((row, index) => (
            <tr key={rowKey ? rowKey(row, index) : index}>
              {columns.map((column) => (
                <td key={column.key} data-align={column.align}>
                  {column.cell ? column.cell(row) : (row as Record<string, ReactNode>)[column.key]}
                </td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
