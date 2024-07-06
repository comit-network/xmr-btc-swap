import { Box, Button, makeStyles, Typography } from '@material-ui/core';
import InfoBox from '../../modal/swap/InfoBox';

const useStyles = makeStyles((theme) => ({
  spacedBox: {
    display: 'flex',
    gap: theme.spacing(1),
  },
}));

const GITHUB_ISSUE_URL =
  'https://github.com/UnstoppableSwap/unstoppableswap-gui/issues/new/choose';
const MATRIX_ROOM_URL = 'https://matrix.to/#/#unstoppableswap:matrix.org';
export const DISCORD_URL = 'https://discord.gg/APJ6rJmq';

export default function ContactInfoBox() {
  const classes = useStyles();

  return (
    <InfoBox
      title="Get in touch"
      mainContent={
        <Typography variant="subtitle2">
          If you need help or just want to reach out to the contributors of this
          project you can open a GitHub issue, join our Matrix room or Discord
        </Typography>
      }
      additionalContent={
        <Box className={classes.spacedBox}>
          <Button
            variant="outlined"
            onClick={() => window.open(GITHUB_ISSUE_URL)}
          >
            Open GitHub issue
          </Button>
          <Button
            variant="outlined"
            onClick={() => window.open(MATRIX_ROOM_URL)}
          >
            Join Matrix room
          </Button>
          <Button variant="outlined" onClick={() => window.open(DISCORD_URL)}>
            Join Discord
          </Button>
        </Box>
      }
      icon={null}
      loading={false}
    />
  );
}
