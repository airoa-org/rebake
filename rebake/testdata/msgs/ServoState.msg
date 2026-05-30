Header header
string[] name
float64[] command_position
float64[] command_velocity
float64[] command_current
float64[] present_position
float64[] present_velocity
float64[] present_effort
float64[] present_temperature
float64[] present_current
float64[] present_motor_shaft_position
float64[] present_driven_shaft_position

uint16[] error_status
string[] message

================================================================================
MSG: std_msgs/Header
# Standard metadata for higher-level stamped data types.
# This is generally used to communicate timestamped data 
# in a particular coordinate frame.
# 
# sequence ID: consecutively increasing ID 
uint32 seq
#Two-integer timestamp that is expressed as:
# * stamp.sec: seconds (stamp_secs) since epoch (in Python the variable is called 'secs')
# * stamp.nsec: nanoseconds since stamp_secs (in Python the variable is called 'nsecs')
# time-handling sugar is provided by the client library
time stamp
#Frame this data is associated with
string frame_id
