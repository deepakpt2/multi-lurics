import { publicMetadata, publicPage } from "@/components/public/public-page";

// Rendered on demand, cached, and dropped by the worker when the content or its URL changes.
export const revalidate = 300;
export const generateMetadata = publicMetadata("artist");
export default publicPage("artist");
