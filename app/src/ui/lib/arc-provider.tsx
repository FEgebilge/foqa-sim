"use client";

/**
 * Lets an app swap the link and image elements Arc components render (kuratlielia/arc-library#13).
 *
 * Without a provider, Arc renders a plain `<a>` and `<img>`, so components work in any React app (Vite, Remix, Astro,
 * plain React). Wrap the app once to use your framework's or your own components instead:
 *
 *   import Link from "next/link";
 *   import { ArcImage as Image } from "@ui/lib/arc-provider";
 *   import { ArcProvider } from "@ui/lib/arc-provider";
 *
 *   <ArcProvider link={Link} image={Image}>{children}</ArcProvider>
 *
 * Any component that accepts these props works, such as a locale-aware Link from next-intl or a React Router Link
 * wrapped to map `href` to `to`.
 *
 * The same provider localises components: `locale` and `timeZone` become every component's defaults, and `messages`
 * replaces their built-in English labels, keyed by component slug. A `labels` prop on one component still wins.
 *
 *   <ArcProvider locale="tr-TR" timeZone="Europe/Istanbul" messages={{ dialog: { close: "Kapat" } }}>{children}</ArcProvider>
 */
import { createContext, createElement, useContext, useMemo, type AnchorHTMLAttributes, type ComponentType, type ImgHTMLAttributes, type ReactNode, type Ref } from "react";

/** The props Arc passes to a link: an href plus ordinary anchor attributes. */
export interface ArcLinkProps extends AnchorHTMLAttributes<HTMLAnchorElement> {
  href: string;
  ref?: Ref<HTMLAnchorElement>;
}

/**
 * The props Arc passes to an image. A subset of next/image's props, so next/image can be passed as is: either `fill`
 * (cover the positioned parent) or `width` and `height`, plus `sizes` and `priority` as hints.
 */
export interface ArcImageProps extends Omit<ImgHTMLAttributes<HTMLImageElement>, "src" | "alt" | "width" | "height" | "loading" | "srcSet"> {
  src: string;
  alt: string;
  width?: number;
  height?: number;
  /** Fills the nearest positioned parent, like next/image's `fill`. */
  fill?: boolean;
  /** Loads the image eagerly with high priority, like next/image's `priority`. */
  priority?: boolean;
  loading?: "eager" | "lazy";
  ref?: Ref<HTMLImageElement>;
}

export type ArcLinkComponent = ComponentType<ArcLinkProps>;
export type ArcImageComponent = ComponentType<ArcImageProps>;

/** The default link: a plain anchor. */
export function ArcAnchor(props: ArcLinkProps) {
  return <a {...props} />;
}

const fillStyle = { position: "absolute", inset: 0, width: "100%", height: "100%" } as const;

/** The default image: a plain, lazily decoded `<img>` that understands `fill` and `priority`. */
export function ArcImg({ alt, fill, priority, loading, style, ...props }: ArcImageProps) {
  // eslint-disable-next-line @next/next/no-img-element -- the framework-free default; pass next/image through ArcProvider instead.
  return <img alt={alt} decoding="async" {...props} loading={loading ?? (priority ? "eager" : "lazy")} fetchPriority={priority ? "high" : props.fetchPriority} style={fill ? { ...fillStyle, ...style } : style} />;
}

/**
 * The label set of every component that reads `messages`, keyed by component slug. Each such component registers its own
 * entry, so `messages` is typed by exactly the components an app has:
 *
 *   declare module "@ui/lib/arc-provider" { interface ArcLabels { dialog: DialogLabels } }
 */
export interface ArcLabels {}

/** Label overrides by component slug; any label a component leaves out keeps its English default. */
export type ArcMessages = { [Slug in keyof ArcLabels]?: Partial<ArcLabels[Slug]> };

interface ArcContextValue {
  link: ArcLinkComponent;
  image: ArcImageComponent;
  locale: string;
  timeZone?: string;
  messages: ArcMessages;
}

const ArcContext = createContext<ArcContextValue>({ link: ArcAnchor, image: ArcImg, locale: "en-US", messages: {} });

export interface ArcProviderProps {
  /** The link component Arc renders for navigation, such as next/link or your own wrapped Link. Defaults to `<a>`. */
  link?: ArcLinkComponent;
  /** The image component Arc renders for photos, such as next/image. Defaults to `<img>`. */
  image?: ArcImageComponent;
  /** BCP 47 locale for numbers, dates, sorting and search, such as "tr-TR". Defaults to "en-US". */
  locale?: string;
  /** IANA time zone for dates, such as "Europe/Istanbul". Components that need one fall back to their own default. */
  timeZone?: string;
  /** Labels that replace the English defaults, by component slug. Merged per component with an outer provider's. */
  messages?: ArcMessages;
  children: ReactNode;
}

/** Sets links, images, locale and labels for every Arc component inside it. Nested providers inherit what they do not set. */
export function ArcProvider({ link, image, locale, timeZone, messages, children }: ArcProviderProps) {
  const parent = useContext(ArcContext);
  const value = useMemo(() => {
    const merged: Record<string, object | undefined> = { ...parent.messages };
    for (const [slug, labels] of Object.entries(messages ?? {}) as [string, object][]) merged[slug] = { ...merged[slug], ...labels };
    return { link: link ?? parent.link, image: image ?? parent.image, locale: locale ?? parent.locale, timeZone: timeZone ?? parent.timeZone, messages: merged as ArcMessages };
  }, [link, image, locale, timeZone, messages, parent]);
  return <ArcContext.Provider value={value}>{children}</ArcContext.Provider>;
}

/** The link component from the nearest ArcProvider, or a plain anchor. */
export const useArcLink = () => useContext(ArcContext).link;
/** The image component from the nearest ArcProvider, or a plain img. */
export const useArcImage = () => useContext(ArcContext).image;

/** The locale and time zone from the nearest ArcProvider: "en-US" and no zone without one. */
export function useArcLocale() {
  const { locale, timeZone } = useContext(ArcContext);
  return { locale, timeZone };
}

/** A component's labels: its English `defaults`, then the provider's `messages[slug]`, then the component's own `labels` prop. */
export function useLabels<Labels extends object>(slug: keyof ArcLabels, defaults: Labels, labels?: Partial<Labels>): Labels {
  const provided = useContext(ArcContext).messages[slug] as Partial<Labels> | undefined;
  return useMemo(() => ({ ...defaults, ...provided, ...labels }), [defaults, provided, labels]);
}

/** What Arc components render for a link: the provider's link component, or a plain anchor. */
export function ArcLink(props: ArcLinkProps) {
  return createElement(useArcLink(), props);
}

/** What Arc components render for an image: the provider's image component, or a plain img. */
export function ArcImage(props: ArcImageProps) {
  return createElement(useArcImage(), props);
}
