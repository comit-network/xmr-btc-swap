import ChevronRightIcon from "@material-ui/icons/ChevronRight";
import ExpandMoreIcon from "@material-ui/icons/ExpandMore";
import TreeItem from "@material-ui/lab/TreeItem";
import TreeView from "@material-ui/lab/TreeView";
import ScrollablePaperTextBox from "./ScrollablePaperTextBox";

interface JsonTreeViewProps {
  data: any;
  label: string;
}

export default function JsonTreeView({ data, label }: JsonTreeViewProps) {
  const renderTree = (nodes: any, parentId: string) => {
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
