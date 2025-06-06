import ChevronRightIcon from "@mui/icons-material/ChevronRight";
import ExpandMoreIcon from "@mui/icons-material/ExpandMore";
import TreeItem from "@mui/lab/TreeItem";
import TreeView from "@mui/lab/TreeView";
import ScrollablePaperTextBox from "./ScrollablePaperTextBox";

interface JsonTreeViewProps {
  data: unknown;
  label: string;
}

export default function JsonTreeView({ data, label }: JsonTreeViewProps) {
  const renderTree = (nodes: unknown, parentId: string) => {
    return Object.keys(nodes).map((key, _) => {
      const nodeId = `${parentId}.${key}`;
      if (typeof nodes[key] === "object" && nodes[key] !== null) {
        return (
          <TreeItem nodeId={nodeId} label={key} key={nodeId}>
            {renderTree(nodes[key], nodeId)}
          </TreeItem>
        );
      }
      return (
        <TreeItem
          nodeId={nodeId}
          label={`${key}: ${nodes[key]}`}
          key={nodeId}
        />
      );
    });
  };

  return (
    <ScrollablePaperTextBox
      title={label}
      copyValue={JSON.stringify(data, null, 4)}
      rows={[
        <TreeView
          key={1}
          defaultCollapseIcon={<ExpandMoreIcon />}
          defaultExpandIcon={<ChevronRightIcon />}
          defaultExpanded={["root"]}
        >
          <TreeItem nodeId="root" label={label}>
            {renderTree(data ?? {}, "root")}
          </TreeItem>
        </TreeView>,
      ]}
    />
  );
}
