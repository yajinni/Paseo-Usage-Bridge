import type { SVGProps } from "react";

type IconProps = SVGProps<SVGSVGElement>;

const Base = ({ children, ...props }: IconProps) => (
  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true" {...props}>
    {children}
  </svg>
);

export const PlusIcon = (props: IconProps) => <Base {...props}><path d="M12 5v14M5 12h14" /></Base>;
export const RefreshIcon = (props: IconProps) => <Base {...props}><path d="M20 11a8 8 0 1 0 2 5.5"/><path d="M20 4v7h-7"/></Base>;
export const UsersIcon = (props: IconProps) => <Base {...props}><path d="M16 21v-2a4 4 0 0 0-4-4H6a4 4 0 0 0-4 4v2"/><circle cx="9" cy="7" r="4"/><path d="M22 21v-2a4 4 0 0 0-3-3.87M16 3.13a4 4 0 0 1 0 7.75"/></Base>;
export const GaugeIcon = (props: IconProps) => <Base {...props}><path d="M3 12a9 9 0 1 1 18 0"/><path d="m12 12 4-4"/><path d="M5.6 19h12.8"/></Base>;
export const LinkIcon = (props: IconProps) => <Base {...props}><path d="M10 13a5 5 0 0 0 7.54.54l3-3a5 5 0 0 0-7.07-7.07l-1.72 1.71"/><path d="M14 11a5 5 0 0 0-7.54-.54l-3 3a5 5 0 0 0 7.07 7.07l1.71-1.71"/></Base>;
export const SettingsIcon = (props: IconProps) => <Base {...props}><circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06-2.83 2.83-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21h-4v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06-2.83-2.83.06-.06A1.65 1.65 0 0 0 4.6 15a1.65 1.65 0 0 0-1.51-1H3v-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06 2.83-2.83.06.06A1.65 1.65 0 0 0 9 4.6a1.65 1.65 0 0 0 1-1.51V3h4v.09A1.65 1.65 0 0 0 15 4.6a1.65 1.65 0 0 0 1.82-.33l.06-.06 2.83 2.83-.06.06A1.65 1.65 0 0 0 19.4 9c.12.37.19.76.2 1.15H21v4h-1.4c-.01.3-.08.59-.2.85Z"/></Base>;
export const CopyIcon = (props: IconProps) => <Base {...props}><rect x="9" y="9" width="13" height="13" rx="2"/><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/></Base>;
export const TrashIcon = (props: IconProps) => <Base {...props}><path d="M3 6h18M8 6V4h8v2M19 6l-1 14H6L5 6M10 11v5M14 11v5"/></Base>;
export const EditIcon = (props: IconProps) => <Base {...props}><path d="M12 20h9"/><path d="M16.5 3.5a2.12 2.12 0 0 1 3 3L8 18l-4 1 1-4Z"/></Base>;
export const ShieldIcon = (props: IconProps) => <Base {...props}><path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10"/><path d="m9 12 2 2 4-4"/></Base>;
export const ChevronIcon = (props: IconProps) => <Base {...props}><path d="m9 18 6-6-6-6"/></Base>;
